#!/usr/bin/env bash
# Build and install the launcher.
#
# No root: the binary goes in ~/.local/bin and the frontend is baked into it,
# so there is nothing else to place. Uninstalling leaves the shell's own
# launcher in charge, which is what the keybind falls back to.
#
# Usage: ./install.sh              install (builds first)
#        ./install.sh --uninstall  remove
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
target="${XDG_BIN_HOME:-$HOME/.local/bin}/caelestia-launcher"

if [[ ${1:-} == --uninstall ]]; then
    "$target" --hide 2>/dev/null || true
    pkill -x caelestia-launcher 2>/dev/null || true
    rm -f "$target"
    echo "Removed $target"
    exit 0
fi

command -v cargo >/dev/null || { echo "cargo not found: install rust first" >&2; exit 1; }
command -v npm >/dev/null || { echo "npm not found: install node first" >&2; exit 1; }

echo ">> Building the frontend"
(cd "$here" && npm install --silent && npm run build --silent)

# A layer surface is what a launcher should be: keyboard to itself, above
# everything, never tiled. It needs a C library that may not be installed, so
# the build checks rather than failing.
features=()
if pkg-config --exists gtk-layer-shell-0; then
    features=(--features layer-shell)
    echo ">> Building with layer-shell support"
else
    echo ">> gtk-layer-shell not found — building without it"
    echo "   The launcher will be an always-on-top window instead of a layer"
    echo "   surface. Install gtk-layer-shell and re-run this for the real thing."
fi

echo ">> Building the launcher"
cargo build --release --manifest-path "$here/src-tauri/Cargo.toml" "${features[@]}"

mkdir -p "$(dirname "$target")"
# Replaced rather than written over: the old one may be running, and writing
# into a mapped executable is how you get ETXTBSY.
install -m755 "$here/src-tauri/target/release/caelestia-launcher" "$target.new"
mv -f "$target.new" "$target"

echo
echo "Installed $target"
echo "  start it:  caelestia-launcher &"
echo "  toggle it: caelestia-launcher --toggle"
echo "  undo:      $here/install.sh --uninstall"
