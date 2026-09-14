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
    for pid in /proc/[0-9]*; do
        pid=${pid#/proc/}
        exe=$(readlink "/proc/$pid/exe" 2>/dev/null) || continue
        case "$exe" in "$target"|"$target (deleted)") kill "$pid" 2>/dev/null ;; esac
    done
    rm -f "$target" "${XDG_RUNTIME_DIR:-/tmp}/caelestia-launcher.sock"
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

# A launcher started from the old binary keeps running against a file that
# no longer exists, holding its webview and the control socket. Stop it
# first, or the new binary refuses to start and the keybind talks to the old
# one for the rest of the session.
stop_running() {
    local pid exe
    for pid in /proc/[0-9]*; do
        pid=${pid#/proc/}
        exe=$(readlink "/proc/$pid/exe" 2>/dev/null) || continue
        case "$exe" in
            "$target"|"$target (deleted)") kill "$pid" 2>/dev/null ;;
        esac
    done
}
stop_running
sleep 0.5

mkdir -p "$(dirname "$target")"
# Replaced rather than written over: writing into a mapped executable is how
# you get ETXTBSY.
install -m755 "$here/src-tauri/target/release/caelestia-launcher" "$target.new"
mv -f "$target.new" "$target"
rm -f "${XDG_RUNTIME_DIR:-/tmp}/caelestia-launcher.sock"

# Back up resident, so the keybind works without waiting for a relogin.
setsid "$target" >/dev/null 2>&1 < /dev/null &

echo
echo "Installed $target"
echo "  start it:  caelestia-launcher &"
echo "  toggle it: caelestia-launcher --toggle"
echo "  undo:      $here/install.sh --uninstall"
