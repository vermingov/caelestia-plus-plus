#!/usr/bin/env bash
# Root half for quickshell on this machine: build it against the installed
# Qt, install it, and keep the scheduler from treating it as a background
# service.
#
# Why build at all: quickshell links against Qt private API. Every prebuilt
# package (repo, AUR, our old release asset) stops loading after a Qt patch
# release with "undefined symbol ... version Qt_6_PRIVATE_API", and a shell
# that is already running keeps working on the old, deleted libraries — so
# the breakage only shows at the next login, as "quickshell crashed". A build
# on the machine itself binds to whatever Qt is there.
#
# The scheduler part: cachyos-ananicy-rules classifies `qs` as a Service
# (nice 10, io best-effort 6), which starves the shell under load. A rule
# override puts it back at foreground priority.
#
# Usage: run as root (pkexec/sudo), directly from the checkout.
#   install.sh                build only when the installed qs fails to load
#   install.sh --force        build and install regardless
#   install.sh --pkg <file>   install this already-built package instead
set -euo pipefail

# Bump whenever ANY root-side file of this feature changes; the shell's
# system scan compares it against /etc/caelestia/quickshell.version.
root_half_version=1

if [[ $EUID -ne 0 ]]; then
    echo "Run as root: sudo $0" >&2
    exit 1
fi

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
target_user=${SUDO_USER:-}
[[ -z "$target_user" && -n "${PKEXEC_UID:-}" ]] && target_user=$(id -nu "$PKEXEC_UID")
[[ -z "$target_user" ]] && target_user=$(stat -c %U "$here")
if [[ -z "$target_user" || "$target_user" == root ]]; then
    echo "Could not determine the target user." >&2
    exit 1
fi
target_home=$(getent passwd "$target_user" | cut -d: -f6)

qs_loads() {
    timeout 20 runuser -u "$target_user" -- env HOME="$target_home" qs --version >/dev/null 2>&1
}

install_scheduler_override() {
    [[ -d /etc/ananicy.d ]] || return 0
    install -Dm644 "$here/quickshell.rules" /etc/ananicy.d/zz-caelestia/quickshell.rules
    systemctl try-reload-or-restart ananicy-cpp.service 2>/dev/null || true
    # ananicy-cpp keeps the last rule it reads for a name, in directory
    # traversal order, which nothing guarantees; check what actually won
    local effective
    effective=$(timeout 10 ananicy-cpp dump rules 2>/dev/null | sed -n '/^{/,$p' | python3 -c '
import json, sys
rules = json.load(sys.stdin)
print(rules.get("qs", {}).get("type", ""))' 2>/dev/null || true)
    if [[ $effective == Chat ]]; then
        echo ":: scheduler override installed (qs is no longer a nice-10 Service)"
    else
        echo "!! scheduler override lost to another qs rule (effective type: ${effective:-unknown}); the shell may still run at background priority" >&2
    fi
}

build_and_install() {
    local pkg=$prebuilt
    if [[ -z $pkg ]]; then
        pkg=$(build_package)
    fi
    echo ":: installing $(basename "$pkg")"
    # --ask=22 auto-answers the conflict removal of quickshell/quickshell-git
    pacman -U --noconfirm --ask=22 "$pkg"
    grep -q '^IgnorePkg.*caelestia++-quickshell' /etc/pacman.conf \
        || sed -i '/^\[options\]/a IgnorePkg = caelestia++-quickshell' /etc/pacman.conf
}

# Prints the path of the freshly built package; progress goes to stderr
build_package() {
    local build_dir="$target_home/.cache/caelestia/quickshell-build"
    echo ":: installing build dependencies" >&2
    pacman -S --needed --noconfirm base-devel cli11 cmake ninja qt6-shadertools spirv-tools vulkan-headers wayland-protocols >&2

    echo ":: building quickshell in $build_dir (a few minutes)" >&2
    runuser -u "$target_user" -- mkdir -p "$build_dir"
    # Everything the PKGBUILD lists in source=() lives beside it, patches included
    local staged=("$here/PKGBUILD")
    local extra
    for extra in "$here"/*.hook "$here"/*.patch; do
        [[ -e "$extra" ]] && staged+=("$extra")
    done
    runuser -u "$target_user" -- cp "${staged[@]}" "$build_dir/"
    # Generic flags: the package may be shared, and a shell gains nothing
    # from -march=native. makepkg refuses root, so drop to the user.
    runuser -u "$target_user" -- env HOME="$target_home" \
        CFLAGS="-march=x86-64 -mtune=generic -O2 -pipe -fno-plt -fexceptions -Wp,-D_FORTIFY_SOURCE=3 -Wformat -Werror=format-security -fstack-clash-protection -fcf-protection" \
        CXXFLAGS="-march=x86-64 -mtune=generic -O2 -pipe -fno-plt -fexceptions -Wp,-D_FORTIFY_SOURCE=3 -Wformat -Werror=format-security -fstack-clash-protection -fcf-protection -Wp,-D_GLIBCXX_ASSERTIONS" \
        bash -c "cd '$build_dir' && makepkg -Cf --skipinteg" >&2

    ls -t "$build_dir"/caelestia++-quickshell-*.pkg.tar.zst | head -1
}

force=0
prebuilt=""
while [[ $# -gt 0 ]]; do
    case $1 in
        --force) force=1 ;;
        --pkg) prebuilt=$(realpath "$2"); shift ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
    shift
done

if [[ $force == 0 && -z $prebuilt ]] && qs_loads; then
    echo ":: installed quickshell loads fine, nothing to rebuild"
else
    build_and_install
    if qs_loads; then
        echo ":: quickshell rebuilt and loading"
    else
        echo "!! the rebuilt quickshell still fails to start; run 'qs --version' for the reason" >&2
        exit 1
    fi
fi

install_scheduler_override

install -d /etc/caelestia
echo "$root_half_version" > /etc/caelestia/quickshell.version
echo ":: done. A running shell keeps using the old binary until it restarts: caelestia shell -d"
