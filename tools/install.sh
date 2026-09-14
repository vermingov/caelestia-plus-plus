#!/usr/bin/env bash
# Install the shell's helper binary.
#
# No root: ~/.local/bin is on PATH, and the shell looks for the binary there.
# Without it the shell falls back to the Python scripts under assets/, so
# removing this changes speed and nothing else.
#
# Usage: ./install.sh            install (builds first)
#        ./install.sh --uninstall   remove, leaving the scripts in charge
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
target="${XDG_BIN_HOME:-$HOME/.local/bin}/caelestia-tools"

if [[ ${1:-} == --uninstall ]]; then
    rm -f "$target"
    echo "Removed $target — the shell will use the Python scripts again."
    exit 0
fi

command -v cargo >/dev/null || { echo "cargo not found: install rust first" >&2; exit 1; }

echo ">> Building"
cargo build --release --manifest-path "$here/Cargo.toml"
install -Dm755 "$here/target/release/caelestia-tools" "$target"

echo "Installed $target"
echo "  undo with: $here/install.sh --uninstall"
