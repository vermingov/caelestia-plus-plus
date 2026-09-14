#!/usr/bin/env bash
# One-time root install for the Redwall per-application firewall.
#
# Installs the enforcement daemon (NFQUEUE), its nftables ruleset, and a systemd
# service that starts it at boot so filtering is active across reboots. The
# Quickshell bar widget is the UI and needs no install; it just talks to the
# daemon's socket once this is running.
#
# Usage: sudo ./install.sh
set -euo pipefail

# Bump on every change to the daemon or the ruleset. The system scan compares
# this against /etc/caelestia/redwall.version and offers to re-run this script,
# which is the only way a daemon fix reaches an already-installed machine.
root_half_version=2

if [[ $EUID -ne 0 ]]; then
    echo "Run as root: sudo $0" >&2
    exit 1
fi

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
uigid=${SUDO_GID:-1000}   # gid allowed to talk to the UI socket (the real user)

echo ">> Installing dependencies"
pacman -S --needed --noconfirm nftables conntrack-tools

echo ">> Deploying daemon to /opt/redwall"
install -d /opt/redwall
install -m644 "$here/README.md" /opt/redwall/README.md 2>/dev/null || true

# The daemon is Rust. It talks to NFQUEUE over a plain netlink socket and has
# no dependencies at all, so there is no venv, no NetfilterQueue build against
# libnetfilter_queue, and no interpreter resident on the packet path. The
# Python implementation is kept beside it as a fallback for machines with no
# Rust toolchain.
daemon_exec=""
if command -v cargo >/dev/null; then
    echo ">> Building redwalld (Rust)"
    build_user=${SUDO_USER:-}
    [[ -z $build_user && -n ${PKEXEC_UID:-} ]] && build_user=$(id -nu "$PKEXEC_UID")
    [[ -z $build_user ]] && build_user=$(stat -c %U "$here")

    if runuser -u "$build_user" -- env -C "$here/redwalld-rs" cargo build --release --offline \
        || runuser -u "$build_user" -- env -C "$here/redwalld-rs" cargo build --release; then
        install -m755 "$here/redwalld-rs/target/release/redwalld" /opt/redwall/redwalld
        daemon_exec="/opt/redwall/redwalld"
        echo "   built and installed /opt/redwall/redwalld"
    else
        echo "!! the Rust build failed; falling back to the Python daemon" >&2
    fi
else
    echo "!! cargo not found; falling back to the Python daemon" >&2
fi

if [[ -z $daemon_exec ]]; then
    install -m755 "$here/redwalld.py" /opt/redwall/redwalld.py
    pacman -S --needed --noconfirm libnetfilter_queue python gcc
    if [[ ! -x /opt/redwall/venv/bin/python ]]; then
        python -m venv /opt/redwall/venv
    fi
    /opt/redwall/venv/bin/pip install --quiet --upgrade pip Cython
    # NetfilterQueue compiles against libnetfilter_queue (needs base-devel/gcc).
    /opt/redwall/venv/bin/pip install --quiet NetfilterQueue
    daemon_exec="/opt/redwall/venv/bin/python /opt/redwall/redwalld.py"
fi

echo ">> Installing nftables ruleset + systemd unit"
install -d /etc/redwall
install -m644 "$here/redwall.nft" /etc/redwall/redwall.nft
sed -e "s/__UIGID__/$uigid/" -e "s|__EXEC__|$daemon_exec|" \
    "$here/redwalld.service" > /etc/systemd/system/redwalld.service

install -d /etc/caelestia
echo "$root_half_version" > /etc/caelestia/redwall.version

echo ">> Enabling service"
systemctl daemon-reload
systemctl enable redwalld.service
# restart (not just start) so re-running the installer applies daemon/ruleset updates
systemctl restart redwalld.service

echo
echo "Redwall installed and running. Verify with:"
echo "    systemctl status redwalld.service"
echo "    sudo nft list table inet redwall"
echo
echo "The bar shield turns active once the shell reconnects to the socket."
echo "New outbound apps now prompt. Manage rules from the shield popup."
echo "To remove everything: sudo $here/uninstall.sh"
