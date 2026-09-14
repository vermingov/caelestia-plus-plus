#!/usr/bin/env bash
# One-time root install for Redguard, the behavioral process protection daemon.
#
# Deploys the daemon and a systemd service that starts it at boot. The
# Quickshell Protection tab is the UI and needs no install. Run via pkexec by
# the shell on first enable, so it works non-interactively on any machine.
set -euo pipefail

root_half_version=3

if [[ $EUID -ne 0 ]]; then
    echo "Run as root: sudo $0" >&2
    exit 1
fi

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# The gid allowed to talk to the UI socket = the real user's primary group.
uigid=${PKEXEC_UID:+$(id -g "$PKEXEC_UID" 2>/dev/null)}
[[ -z "${uigid:-}" ]] && uigid=${SUDO_GID:-}
[[ -z "${uigid:-}" ]] && uigid=$(stat -c %g "$here")

echo ">> Deploying daemon to /opt/redguard"
install -d /opt/redguard
install -m644 "$here/README.md" /opt/redguard/README.md 2>/dev/null || true

# The daemon is Rust, built out of the workspace it shares with redwall: same
# JSON, same socket server, same /proc reads. Watching every exec on the
# machine is not work for an interpreter, and a monitor that costs nothing is
# a monitor people leave switched on.
#
# The Python daemon is what installs by default. Rust is opt-in until the Rust daemon has been verified against a live proc
# connector: it runs as root and can SIGKILL what it does not like. See the
# note in ../redwall/install.sh.
daemon_exec=""
if [[ ${REDGUARD_RUST:-0} == 1 ]] && binary=$("$here/../rust/build.sh" redguardd); then
    install -m755 "$binary" /opt/redguard/redguardd
    daemon_exec="/opt/redguard/redguardd"
    echo "   installed /opt/redguard/redguardd"
elif [[ ${REDGUARD_RUST:-0} == 1 ]]; then
    echo "!! the Rust build failed; falling back to the Python daemon" >&2
fi

if [[ -z $daemon_exec ]]; then
    install -m755 "$here/redguardd.py" /opt/redguard/redguardd.py
    daemon_exec="/usr/bin/python3 /opt/redguard/redguardd.py"
fi

echo ">> Installing systemd unit"
sed -e "s/__UIGID__/$uigid/" -e "s|__EXEC__|$daemon_exec|" \
    "$here/redguardd.service" > /etc/systemd/system/redguardd.service

install -d /etc/caelestia
echo "$root_half_version" > /etc/caelestia/redguard.version

echo ">> Enabling service"
systemctl daemon-reload
systemctl enable redguardd.service
# restart (not just start) so re-running the installer applies daemon updates
systemctl restart redguardd.service

echo
echo "Redguard installed and running. Verify with:"
echo "    systemctl status redguardd.service"
echo
echo "The Protection tab turns active once the shell reconnects to the socket."
echo "Suspicious execs now freeze and prompt. Manage rules from the shield popup."
echo "To remove everything: sudo $here/uninstall.sh"
