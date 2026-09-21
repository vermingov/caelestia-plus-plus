#!/usr/bin/env python3
"""Take Quickshell and the old QML shell off a machine cae has taken over.

`standalone.py` stops the session starting Quickshell; this is the step after,
and it is the one that is hard to undo, so it refuses to do anything until it
has watched cae draw the desktop itself. What goes:

  * `caelestia++-shell`      the QML shell, in /etc/xdg/quickshell/caelestia
  * `caelestia++-quickshell` the runtime that loaded it, /usr/bin/qs
  * the scheduler override and version stamp its root half left behind
  * `caelestia-bar` and `caelestia-launcher`, the Tauri pair cae replaced

What stays, and why:

  * `caelestia++-cli` — `caelestia screenshot -r` is still on two keybinds,
    and the colour scheme is still its work.
  * nothing of the QML shell — it left the repository once cae had replaced
    all of it, so there is none here to remove and none to go back to. `cae
    revert` is the way back, and it brings upstream caelestia's own shell.

    retire.py            say what would go, take nothing
    retire.py --apply    take it, in one root call

Putting it back is `cae revert`, or pacman on the three packages in
~/.cache/caelestia/packages.
"""

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

# The QML shell first: it is what depends on the runtime, and pacman will not
# take a package something else still needs.
PACKAGES = ["caelestia++-shell", "caelestia++-quickshell"]
# Removed with the packages, never on its own: the keybinds still call it.
KEPT = "caelestia++-cli"

# What the quickshell root half put outside of pacman's knowledge. Leaving the
# version stamp behind is what makes `cae` go on offering the half an upgrade
# for a package that is no longer installed.
ROOT_LEFTOVERS = [
    Path("/etc/ananicy.d/zz-caelestia/quickshell.rules"),
    Path("/etc/caelestia/quickshell.version"),
]

# The Tauri bar and its launcher. cae draws both; the launcher one still runs
# if something starts it, finds cae holding the socket, and exits.
OURS = Path.home() / ".local/bin"
STALE_BINARIES = [OURS / "caelestia-bar", OURS / "caelestia-launcher"]

UNIT = "cae-shell.service"
# cae kept Quickshell's layer namespaces, so that the compositor rules written
# for them still apply. That means the name on a layer says nothing about
# which shell put it there, and the pid beside it is the only thing that does.
DRAWING = "caelestia-bar"


def ran(*command, **kept):
    return subprocess.run(command, capture_output=True, text=True, check=False, **kept)


def installed(package):
    """The version pacman has, or None."""
    done = ran("pacman", "-Q", package)
    return done.stdout.split()[1] if done.returncode == 0 else None


def cae_is_drawing():
    """Whether cae is enabled, running, and actually on the screen.

    All three, because each answers a different way of being wrong: not
    enabled is a desktop with nothing on it at the next login, not running is
    one now, and neither shows a unit that starts and exits.
    """
    if ran("systemctl", "--user", "is-enabled", UNIT).returncode != 0:
        return False, f"{UNIT} is not enabled — run standalone.py --apply first"
    if ran("systemctl", "--user", "is-active", UNIT).returncode != 0:
        return False, f"{UNIT} is not running — `cae restart`, then try again"

    mine = ran("systemctl", "--user", "show", "-p", "MainPID", "--value", UNIT).stdout.strip()
    if not mine or mine == "0":
        return False, f"{UNIT} has no process — look at `journalctl --user -u cae-shell`"

    # `namespace: caelestia-bar, pid: 1234` — the bar has to be on the screen
    # and it has to be this unit's, or what is drawing it is Quickshell.
    for line in ran("hyprctl", "layers").stdout.splitlines():
        if DRAWING in line and f"pid: {mine}" in line:
            return True, ""
    return False, f"the bar on screen is not {UNIT}'s (pid {mine}) — Quickshell is still drawing it"


def others_needing_quickshell():
    """Anything outside this set that pacman says needs the runtime.

    A machine may have had Quickshell before Caelestia did, or somebody may
    have written their own shell against it. Taking it out from under that is
    not this script's business.
    """
    done = ran("pacman", "-Qi", "caelestia++-quickshell")
    for line in done.stdout.splitlines():
        if not line.startswith("Required By"):
            continue
        needed = line.split(":", 1)[1].split()
        return [name for name in needed if name not in PACKAGES and name != "None"]
    return []


# Where `cae revert` looks for a package to put back. It is the only way
# back once pacman no longer has these, and it is routinely empty: pacman's
# own cache is cleaned on a timer on most machines, and a package installed
# from a release download was never in it to begin with.
PACKAGE_CACHE = Path.home() / ".cache/caelestia/packages"
RELEASES = "https://api.github.com/repos/vermingov/caelestia-plus-plus/releases/latest"


def kept_for_a_way_back(packages):
    """Makes sure each package can be reinstalled before it is removed.

    Taking something off a machine with no way to put it back is not a
    migration, it is a one-way door. pacman keeps nothing once a package is
    gone, so a copy goes in the cache `cae revert` reads — from pacman's own
    cache if it still has one, and from the release otherwise.

    Returns what could not be secured, which is what stops the removal.
    """
    PACKAGE_CACHE.mkdir(parents=True, exist_ok=True)
    have = {path.name.rsplit("-", 3)[0] for path in PACKAGE_CACHE.glob("*.pkg.tar.zst")}
    wanted = [name for name in packages if name not in have]
    if not wanted:
        return []

    for name in list(wanted):
        for cached in Path("/var/cache/pacman/pkg").glob(f"{name}-*.pkg.tar.zst"):
            shutil.copy2(cached, PACKAGE_CACHE / cached.name)
            print(f"    kept {cached.name} (from pacman's cache)")
            wanted.remove(name)
            break
    if not wanted:
        return []

    # The releases carry every package as an asset, which is where a machine
    # that installed from one got them in the first place.
    try:
        import json
        import urllib.request

        with urllib.request.urlopen(RELEASES, timeout=25) as answer:
            assets = json.load(answer).get("assets", [])
    except Exception as trouble:
        print(f"    could not reach the releases: {trouble}")
        return wanted

    for name in list(wanted):
        for asset in assets:
            label = asset.get("name", "").replace("%2B", "+").replace("%2b", "+")
            if not label.startswith(f"{name}-") or not label.endswith(".pkg.tar.zst"):
                continue
            try:
                urllib.request.urlretrieve(asset["browser_download_url"], PACKAGE_CACHE / label)
            except Exception as trouble:
                print(f"    could not download {label}: {trouble}")
                break
            print(f"    kept {label} (from the latest release)")
            wanted.remove(name)
            break
    return wanted


def plan():
    """Everything that would go, as (what, how it is described) pairs."""
    going = [(package, f"package {package} {version}")
             for package in PACKAGES if (version := installed(package))]
    going += [(path, f"file {path}") for path in ROOT_LEFTOVERS if path.exists()]
    going += [(path, f"binary {path}") for path in STALE_BINARIES if path.exists()]
    return going


def as_root(going):
    """One script, one password prompt — the same bargain `cae` strikes.

    `pacman -R` without `-s`: the CLI is a dependency of the QML shell and
    would go with it under `-Rs`, and two keybinds still call it.
    """
    lines = ["set -e"]
    packages = [name for name, _ in going if isinstance(name, str)]
    if packages:
        lines.append("pacman -R --noconfirm " + " ".join(packages))
    for path in (path for path, _ in going if isinstance(path, Path) and path in ROOT_LEFTOVERS):
        lines.append(f"rm -f {path}")
    # An empty override directory left behind reads as a setting that is still
    # there. It only goes if nothing else put anything in it.
    lines.append("rmdir --ignore-fail-on-non-empty /etc/ananicy.d/zz-caelestia 2>/dev/null || true")
    return "\n".join(lines)


def main():
    asked = argparse.ArgumentParser(description=__doc__,
                                    formatter_class=argparse.RawDescriptionHelpFormatter)
    asked.add_argument("--apply", action="store_true", help="take it, rather than say what would go")
    args = asked.parse_args()

    going = plan()
    if not going:
        # The words `cae`'s migration watches for. Nothing here to do is the
        # finished state, not a failure.
        print("Quickshell is already retired.")
        return

    drawing, why = cae_is_drawing()
    if not drawing:
        sys.exit(f"retire: {why}")

    others = others_needing_quickshell()
    if others:
        sys.exit("retire: these still need the Quickshell runtime, so it stays: " + ", ".join(others))

    print("These go:\n")
    for _, described in going:
        print(f"    {described}")
    print(f"\n{KEPT} stays: two keybinds still call `caelestia screenshot`.")
    print("There is no QML here to remove: it left the repository once cae had")
    print("replaced all of it. `cae revert` is the way back, and brings its own shell.")

    if not args.apply:
        print("\nNothing has been taken. `--apply` takes it.")
        return

    # Before anything is taken, make sure it can be put back.
    packages = [name for name, _ in going if isinstance(name, str)]
    print("\nKeeping a copy of each package, so `cae revert` has one to put back:")
    stranded = kept_for_a_way_back(packages)
    if stranded:
        sys.exit("retire: no way back for " + ", ".join(stranded) + ".\n"
                 "        Nothing was removed. pacman has no copy and the release could\n"
                 "        not be reached; try again when it can.")

    script = as_root(going)
    print("\nAsking for root once, for pacman and the two files it does not own.")
    if subprocess.run(["sudo", "bash", "-c", script], check=False).returncode != 0:
        sys.exit("retire: the root half did not finish; nothing else was touched")

    for path in (path for path, _ in going if isinstance(path, Path) and path in STALE_BINARIES):
        path.unlink(missing_ok=True)

    # A shell that is drawing does not stop being right because its packages
    # went, but say so plainly rather than leave it to be noticed.
    drawing, why = cae_is_drawing()
    print("\nQuickshell is retired." if drawing else f"\nQuickshell is retired, but {why}")
    if shutil.which("qs"):
        print("`qs` is still on PATH — something else provides it.")


if __name__ == "__main__":
    main()
