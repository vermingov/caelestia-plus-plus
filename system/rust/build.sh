#!/usr/bin/env bash
# Build one of the privileged daemons and print where the binary landed.
#
# Both installers run as root (sudo or pkexec) but must not build as root:
# cargo would write its target directory and registry cache into the repo as
# root and leave the user unable to build again. So the compile is handed back
# to whoever actually owns the checkout.
#
# Usage: build.sh redwalld      # prints /path/to/target/release/redwalld
# Progress goes to stderr, the path to stdout, so callers can capture it.
set -euo pipefail

pkg=${1:?usage: build.sh <redwalld|redguardd>}
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

command -v cargo >/dev/null || { echo "cargo not found" >&2; exit 1; }

# Who owns this checkout, and so who should own the build artefacts.
build_user=${SUDO_USER:-}
[[ -z $build_user && -n ${PKEXEC_UID:-} ]] && build_user=$(id -nu "$PKEXEC_UID")
[[ -z $build_user ]] && build_user=$(stat -c %U "$here")

run_cargo() {
    if [[ $EUID -eq 0 && $build_user != root ]]; then
        runuser -u "$build_user" -- env -C "$here" cargo "$@"
    else
        env -C "$here" cargo "$@"
    fi
}

echo ">> Building $pkg (Rust)" >&2
# Offline first: a machine with the toolchain but no network still builds,
# and neither daemon has a dependency to fetch anyway.
run_cargo build --release --offline -p "$pkg" >&2 \
    || run_cargo build --release -p "$pkg" >&2

binary="$here/target/release/$pkg"
[[ -x $binary ]] || { echo "$pkg did not build" >&2; exit 1; }
echo "$binary"
