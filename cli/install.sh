#!/usr/bin/env bash
# Put the fast CLI in front of the Python one.
#
# No root: ~/.local/bin comes before /usr/bin on PATH, so a binary there is
# what every keybind and every `Quickshell.execDetached(["caelestia", …])`
# finds. The packaged Python CLI stays exactly where it is, and this binary
# hands it everything it does not implement itself.
#
# Usage: ./install.sh            install (builds first)
#        ./install.sh --uninstall   remove, leaving the Python CLI in charge
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
target="${XDG_BIN_HOME:-$HOME/.local/bin}/caelestia"

if [[ ${1:-} == --uninstall ]]; then
    rm -f "$target"
    hash -r 2>/dev/null || true
    echo "Removed $target — $(command -v caelestia || echo 'no caelestia on PATH') is in charge again."
    exit 0
fi

command -v cargo >/dev/null || { echo "cargo not found: install rust first" >&2; exit 1; }

echo ">> Building"
cargo build --release --manifest-path "$here/Cargo.toml"

# Never shadow ourselves: if PATH resolves caelestia to the binary we are
# about to write, the fallback would recurse.
python_cli=$(PATH=${PATH//$HOME\/.local\/bin:/} command -v caelestia || true)
if [[ -z $python_cli ]]; then
    echo "!! no packaged caelestia found on PATH; install caelestia-cli first" >&2
    exit 1
fi

install -Dm755 "$here/target/release/caelestia" "$target"

echo
echo "Installed $target"
echo "  falls back to: $python_cli"
echo "  undo with:     $here/install.sh --uninstall"

# A release can only reach a machine whose `cae` predates migrations through
# something it runs out of the new checkout, and this is one of the few: the
# updater builds the CLI from whatever it has just pulled. If that updater
# cannot offer migrations, it is about to finish and report the machine up to
# date while leaving the desktop on Quickshell, so say so where it will be
# seen — its own output is thrown away.
if [[ -z ${CAE_CAN_MIGRATE:-} ]] && command -v notify-send >/dev/null; then
    root=$(cd "$here/.." && pwd)
    if python3 - "$root" 2>/dev/null <<'PENDING'
import json, subprocess, sys, os
root = sys.argv[1]
steps = json.load(open(f"{root}/release.json")).get("migrations", [])
env = {**os.environ, "SHELL_DIR": root}
# Pending means at least one step whose own test says it is not done yet.
sys.exit(0 if any(
    step.get("done") and subprocess.run(["bash", "-c", step["done"]], env=env,
                                        capture_output=True).returncode != 0
    for step in steps) else 1)
PENDING
    then
        notify-send -u critical -a Caelestia++ "Caelestia++ has one step left" \
            "This release moves the desktop off Quickshell. Run 'cae' once more to finish it." || true
    fi
fi
