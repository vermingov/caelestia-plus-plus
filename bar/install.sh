#!/usr/bin/env bash
# Build and install the bar, and the launcher that now lives inside it.
#
# No root: the binaries go in ~/.local/bin with the frontend baked in.
# Uninstalling leaves the shell's own bar and launcher in charge.
#
# Usage: ./install.sh              install (builds first)
#        ./install.sh --uninstall  remove
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
bindir="${XDG_BIN_HOME:-$HOME/.local/bin}"
target="$bindir/caelestia-bar"
# The launcher is a window of the bar now; this is the client that asks for
# it. Installed from here so the two can never be out of step.
client="$bindir/caelestia-launcher"
# What the shell reads to decide whether the installed binaries match the
# checkout, and whether they were removed on purpose. Without the stamp it
# would rebuild on every startup; without the opt-out it would undo an
# uninstall the next time the shell came up.
state="${XDG_STATE_HOME:-$HOME/.local/state}/caelestia"
stamp="$state/bar-built-from"
optout="$state/bar-optout"

stop_running() {
    local pid exe
    # By executable rather than by name: `comm` is capped at fifteen
    # characters, so "caelestia-launcher" never matches a `pkill -x`, and a
    # standalone launcher left from before the merge would survive to fight
    # the bar for the socket.
    for pid in /proc/[0-9]*; do
        pid=${pid#/proc/}
        exe=$(readlink "/proc/$pid/exe" 2>/dev/null) || continue
        case "$exe" in
            "$target"|"$target (deleted)") kill "$pid" 2>/dev/null ;;
            "$client"|"$client (deleted)") kill "$pid" 2>/dev/null ;;
        esac
    done
    # The visualiser's recorder is a child, and killing its parent does not
    # always take it with it — an orphan left holding the sink monitor is a
    # process per install that nothing will ever clean up.
    pkill -x pw-record 2>/dev/null || true
    rm -f "${XDG_RUNTIME_DIR:-/tmp}/caelestia-launcher.sock"
}

if [[ ${1:-} == --uninstall ]]; then
    stop_running
    rm -f "$target" "$client" "$stamp"
    mkdir -p "$state" && : > "$optout"
    echo "Removed $target and $client"
    echo "The shell's own bar and launcher are back in charge."
    exit 0
fi

command -v cargo >/dev/null || { echo "cargo not found: install rust first" >&2; exit 1; }
command -v npm >/dev/null || { echo "npm not found: install node first" >&2; exit 1; }

echo ">> Building the frontend"
(cd "$here" && npm install --silent && npm run build --silent)

features=()
if pkg-config --exists gtk-layer-shell-0; then
    features=(--features layer-shell)
    echo ">> Building with layer-shell support"
else
    echo ">> gtk-layer-shell not found — the bar will not reserve its strip"
fi

echo ">> Building the bar"
cargo build --release --manifest-path "$here/src-tauri/Cargo.toml" "${features[@]}"

mkdir -p "$(dirname "$target")"
install -m755 "$here/src-tauri/target/release/caelestia-bar" "$target.new"
mv -f "$target.new" "$target"
install -m755 "$here/src-tauri/target/release/caelestia-launcher" "$client.new"
mv -f "$client.new" "$client"

# Stopped only once the new binary is where the old one was. The shell
# respawns the bar six tenths of a second after it exits, and it used to be
# stopped first: the respawn found the old file still in place, started that,
# and the rename landed a moment later — so every install and every update
# left the previous build running until something else restarted it. The
# rename is atomic and the running bar keeps the file it was started from, so
# swapping under it is safe; stop_running knows that process by its
# "(deleted)" executable.
stop_running

mkdir -p "$state"
rm -f "$optout"
git -C "$here/.." rev-parse HEAD 2>/dev/null > "$stamp" || rm -f "$stamp"

# Not started here. The shell owns the bar: it runs it as a child so the two
# come and go together, and it respawns one that exits — which is exactly what
# stopping the old one above asks it to do. Started here as well, with setsid,
# there would be two, and the detached one would outlive the shell.
if ! pgrep -x qs >/dev/null 2>&1; then
    echo "  (the shell is not running — it will start the bar when it does)"
fi

echo
echo "Installed $target"
echo "Installed $client (asks the bar for the launcher)"
echo "  undo: $here/install.sh --uninstall"
