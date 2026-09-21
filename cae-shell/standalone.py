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
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent

HYPR = Path.home() / ".config/hypr/hyprland"
KEPT = ".before-cae"

# Where our own binaries are installed. A keybind is run by the compositor,
# which was started by a display manager and inherited a PATH from a login
# shell that never read anybody's profile — so ~/.local/bin is routinely not
# on it, and a keybind saying `cae-shell launcher` does nothing at all, with
# nowhere for it to say so. The Quickshell globals these replace went over an
# IPC socket and never needed a PATH, which is why nothing warned of it.
BIN = Path(os.environ.get("XDG_BIN_HOME") or Path.home() / ".local/bin")


def spell(command):
    """A command written so that any PATH can run it.

    Ours by absolute path; a packaged one — `caelestia` — left alone, since
    it lives where every PATH looks and hard-coding /usr/bin would break a
    machine that has it somewhere else.
    """
    name, _, rest = command.partition(" ")
    ours = BIN / name
    if not ours.is_file():
        return command
    return f"{ours} {rest}".rstrip()


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


# What the session runs in Quickshell's place. Only execs.lua gets it, and
# only if it has the line this replaces.
QUICKSHELL_EXEC = 'hl.exec_cmd("caelestia shell -d")'
STARTS_NAME = "cae-session"


def changed(text):
    """The file as it would be, and what was done to it, line by line."""
    out, notes = [], []
    for number, line in enumerate(text.splitlines(keepends=True), 1):
        was = line
        for starts, why in STOPPED:
            if starts in line and not line.lstrip().startswith("--"):
                line = line.replace(starts, f"-- {why}: {starts}")
        for name, command in VERBS.items():
            wanted = spell(command)
            shortcut = f'hl.dsp.global("caelestia:{name}")'
            if shortcut in line:
                line = line.replace(shortcut, f'hl.dsp.exec_cmd("{wanted}")')
            # An earlier version wrote these by bare name, which the
            # compositor's PATH cannot find. Same keybind, spelled so it runs.
            elif wanted != command and f'"{command}"' in line:
                line = line.replace(f'"{command}"', f'"{wanted}"')
        if line != was:
            notes.append((number, was.rstrip(), line.rstrip()))
        out.append(line)

    # Stopping Quickshell is only half of it: something has to start cae. A
    # session run by uwsm activates graphical-session.target and the unit is
    # pulled in by it, but a Hyprland started from a display manager
    # activates nothing, and the unit sits enabled and never runs. This line
    # covers both, and goes in whether or not the Quickshell line above has
    # already been commented out — a machine left half moved by an earlier
    # version has nothing else to add it.
    at = next((i for i, line in enumerate(out) if QUICKSHELL_EXEC in line), None)
    starts = f'hl.exec_cmd("{spell(STARTS_NAME)}")'
    if at is not None and not any(STARTS_NAME in line for line in out):
        indent = out[at][: len(out[at]) - len(out[at].lstrip())]
        if not out[at].endswith("\n"):
            out[at] += "\n"
        added = f"{indent}{starts}\n"
        out.insert(at + 1, added)
        notes.append((at + 2, "", added.rstrip()))

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


def put_back():
    """Restores the kept copies and disables the unit. Returns how many.

    The unit goes too, or the next session would start cae twice: once from
    systemd and once from the line just put back.
    """
    restored = 0
    for path in files():
        kept = path.with_suffix(path.suffix + KEPT)
        if not kept.is_file():
            print(f"{path.name}: no copy kept, left alone")
            continue
        shutil.move(kept, path)
        print(f"{path.name}: put back")
        restored += 1
    subprocess.run(["systemctl", "--user", "disable", UNIT], capture_output=True, check=False)
    print(f"{UNIT}: disabled")
    return restored


def main():
    asked = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    asked.add_argument("--apply", action="store_true", help="make the changes")
    asked.add_argument("--now", action="store_true", help="and hand the running session over")
    asked.add_argument("--undo", action="store_true", help="put the kept copies back")
    args = asked.parse_args()
    args.apply = args.apply or args.now

    if args.undo:
        put_back()
        print("\nReload Hyprland, or log in again, for it to take.")
        return

    # Worked out and shown before anything is written. What the session is
    # told to start has to be installed first — see below — so this pass only
    # reads.
    planned, total = [], 0
    for path in files():
        after, notes = changed(path.read_text())
        total += len(notes)
        print(f"\n{path.name}: {len(notes)} lines")
        for number, was, now in notes:
            if was:
                print(f"  {number:>4}  - {was.strip()}")
                print(f"        + {now.strip()}")
            else:
                print(f"  {number:>4}  + {now.strip()}")
        if notes:
            planned.append((path, after))

    # Switched-over files with no unit to start anything is the state an
    # earlier version could leave behind, and it is the one that costs a
    # login: the session has been told not to start Quickshell and nothing
    # has been put in its place. Finish it if the unit will install, and put
    # Quickshell back if it will not — never leave it standing.
    half_done = not total and not unit_enabled()
    if half_done:
        print(f"\nThe files are switched over but {UNIT} is not enabled — half done.")
        print("A desktop in this state comes up empty at the next login.")
        if not args.apply:
            print("`--apply` finishes it, or puts Quickshell back if it cannot.")
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
    # its own is a session with no shell in it, or two. So the unit goes
    # first and the files are only written once it is there — a shell that
    # will not build used to leave the session told not to start Quickshell
    # and nothing put in its place, which is a desktop that comes up empty at
    # the next login and says nothing about why.
    print("\ncae-shell.service:")
    started = subprocess.run(["bash", str(HERE / "install.sh"), "--standalone"], check=False)
    if started.returncode != 0:
        if not half_done:
            sys.exit("standalone: the unit could not be installed, so the session was left alone.\n"
                     "           Nothing needs putting back. Fix the build and run this again.")
        # Already half moved before this run, and the shell still will not
        # build: put Quickshell back rather than leave a desktop that comes
        # up empty.
        restored = put_back()
        sys.exit(f"standalone: the unit could not be installed, and the session was already\n"
                 f"           half moved. Quickshell starts it again ({restored} file(s) put back).\n"
                 f"           Fix the build and run this again.")

    written = []
    try:
        for path, after in planned:
            kept = path.with_suffix(path.suffix + KEPT)
            if not kept.exists():
                shutil.copy2(path, kept)
            path.write_text(after)
            written.append(path)
    except OSError as trouble:
        for path in written:
            kept = path.with_suffix(path.suffix + KEPT)
            if kept.is_file():
                shutil.copy2(kept, path)
        subprocess.run(["systemctl", "--user", "disable", UNIT], capture_output=True, check=False)
        sys.exit(f"standalone: {trouble}\n           The session was put back as it was.")

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
