#!/usr/bin/env python3
"""Point the session at cae, or put it back the way it was.

The shell is cae's; what is still Quickshell's is the session that starts it
and the keys that reach it. Both are two files of the user's own Lua, so this
changes them by exact lines and refuses anything it does not recognise —
there is no pattern here loose enough to eat a line it was not meant to.

    standalone.py            say what would change, change nothing
    standalone.py --apply    change it, keeping a copy of each file first
    standalone.py --now      hand the running session over as well
    standalone.py --undo     put the copies back

`--apply` takes effect at the next login, which is on purpose: it is meant to
be read before it is lived in. `--now` does not wait — it stops the running
Quickshell and starts the unit in its place, and puts Quickshell back by
itself if cae does not come up.
"""

import argparse
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent

HYPR = Path.home() / ".config/hypr/hyprland"
KEPT = ".before-cae"

# Each Quickshell global, and the command that reaches cae instead.
VERBS = {
    "launcher": "cae-shell launcher",
    "session": "cae-shell session",
    "sidebar": "cae-shell centre",
    "clearNotifs": "cae-shell notifs clear",
    "showall": "cae-shell showall",
    "lock": "cae-shell lock",
    "brightnessUp": "cae-shell brightness up",
    "brightnessDown": "cae-shell brightness down",
    "mediaToggle": "cae-shell media play",
    "mediaNext": "cae-shell media next",
    "mediaPrev": "cae-shell media previous",
    "mediaStop": "cae-shell media stop",
    "screenshot": "caelestia screenshot -r",
    "screenshotFreeze": "caelestia screenshot -r -f",
}

# The line that starts Quickshell. It is commented out rather than repointed:
# `cae-shell.service` is what starts cae, so that the shell has one owner,
# comes back if it falls over, and has a log of its own.
# Each line the session no longer wants, and why it is commented out.
STOPPED = [
    ('hl.exec_cmd("caelestia shell -d")', "cae starts from cae-shell.service now"),
    # cae answers `caelestia-launcher.sock` itself, so the Tauri launcher
    # starts, finds the socket taken and exits. A resident web view for
    # nothing.
    ('hl.exec_cmd("caelestia-launcher")', "cae draws the launcher now"),
]


def changed(text):
    """The file as it would be, and what was done to it, line by line."""
    out, notes = [], []
    for number, line in enumerate(text.splitlines(keepends=True), 1):
        was = line
        for starts, why in STOPPED:
            if starts in line and not line.lstrip().startswith("--"):
                line = line.replace(starts, f"-- {why}: {starts}")
        for name, command in VERBS.items():
            shortcut = f'hl.dsp.global("caelestia:{name}")'
            if shortcut in line:
                line = line.replace(shortcut, f'hl.dsp.exec_cmd("{command}")')
        if line != was:
            notes.append((number, was.rstrip(), line.rstrip()))
        out.append(line)
    return "".join(out), notes


def files():
    for name in ("execs.lua", "keybinds.lua"):
        path = HYPR / name
        if not path.is_file():
            sys.exit(f"standalone: {path} is not there")
        yield path


QUICKSHELL = "qs -c caelestia -n -d"
DRAWING = "caelestia-bar"
UNIT = "cae-shell.service"


def unit_enabled():
    """Whether systemd will start cae with the session.

    Asked separately from the files, because the two halves can come apart:
    a session that no longer starts Quickshell and no unit to start cae in
    its place is a desktop with nothing on it, and that is exactly the state
    worth noticing rather than calling finished.
    """
    done = subprocess.run(["systemctl", "--user", "is-enabled", UNIT], capture_output=True, check=False)
    return done.returncode == 0


def shell_is_drawing():
    """Whether anything has put the bar on the screen."""
    layers = subprocess.run(["hyprctl", "layers"], capture_output=True, text=True, check=False)
    return DRAWING in layers.stdout


def hand_over():
    """Stops Quickshell and starts the unit in its place, here and now.

    The two cannot overlap: cae is one process per session, and the one
    Quickshell started holds the lock until it goes. If the unit does not
    put a bar on the screen within a few seconds, Quickshell is started
    again — a desktop with nothing on it is not a thing to leave somebody.
    """
    found = subprocess.run(["pgrep", "-xf", QUICKSHELL], capture_output=True, text=True, check=False)
    for pid in found.stdout.split():
        subprocess.run(["kill", pid], check=False)
    for _ in range(40):
        if subprocess.run(["pgrep", "-x", "qs"], capture_output=True, check=False).returncode != 0:
            break
        time.sleep(0.25)

    subprocess.run(["systemctl", "--user", "start", "cae-shell.service"], check=False)
    for _ in range(40):
        if shell_is_drawing():
            print("\ncae is drawing the desktop. Quickshell is not running.")
            return
        time.sleep(0.25)

    print("\ncae did not come up — putting Quickshell back.")
    subprocess.run(["systemctl", "--user", "stop", "cae-shell.service"], check=False)
    subprocess.Popen(["setsid", "-f", "caelestia", "shell", "-d"])
    sys.exit("standalone: look at `journalctl --user -u cae-shell` before trying again")


def main():
    asked = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    asked.add_argument("--apply", action="store_true", help="make the changes")
    asked.add_argument("--now", action="store_true", help="and hand the running session over")
    asked.add_argument("--undo", action="store_true", help="put the kept copies back")
    args = asked.parse_args()
    args.apply = args.apply or args.now

    if args.undo:
        for path in files():
            kept = path.with_suffix(path.suffix + KEPT)
            if not kept.is_file():
                print(f"{path.name}: no copy kept, left alone")
                continue
            shutil.move(kept, path)
            print(f"{path.name}: put back")
        # The unit goes too, or the next session would start cae twice: once
        # from systemd and once from the line just put back.
        subprocess.run(["systemctl", "--user", "disable", "cae-shell.service"], capture_output=True, check=False)
        print("cae-shell.service: disabled")
        print("\nReload Hyprland, or log in again, for it to take.")
        return

    total = 0
    for path in files():
        text = path.read_text()
        after, notes = changed(text)
        total += len(notes)
        print(f"\n{path.name}: {len(notes)} lines")
        for number, was, now in notes:
            print(f"  {number:>4}  - {was.strip()}")
            print(f"        + {now.strip()}")
        if args.apply and notes:
            kept = path.with_suffix(path.suffix + KEPT)
            if not kept.exists():
                shutil.copy2(path, kept)
            path.write_text(after)

    if not total and not unit_enabled():
        print(f"\nThe files are switched over but {UNIT} is not enabled — half done.")
        if not args.apply:
            print("`--apply` finishes it.")
            return
        total = -1  # nothing in the files to change, but the unit still wants installing

    if not total:
        print("\nNothing left to change: the session already points at cae.")
        if args.now:
            hand_over()
        return
    if not args.apply:
        print(f"\n{total} lines would change, and cae-shell.service would be installed")
        print("and enabled. Nothing has been. `--apply` does it.")
        return

    # The unit and the commented-out line have to move together: either on
    # its own is a session with no shell in it, or two.
    print("\ncae-shell.service:")
    started = subprocess.run(["bash", str(HERE / "install.sh"), "--standalone"], check=False)
    if started.returncode != 0:
        sys.exit("standalone: the unit could not be installed; run --undo to put the files back")

    if args.now:
        hand_over()

    if total < 0:
        print(f"\n{UNIT} installed and enabled; the files were already switched over.")
        if args.now:
            hand_over()
        return

    print(f"\nChanged {total} lines. A copy of each file is beside it as *{KEPT};")
    print("`standalone.py --undo` puts them back and disables the unit.")
    print("\nIt takes effect at the next login. Before then, worth doing:")
    print("  cae restart   — wakes every piece cae has taken over, with Quickshell")
    print("                  still running to fall back on if one of them is wrong")


if __name__ == "__main__":
    main()
