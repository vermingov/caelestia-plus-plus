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
