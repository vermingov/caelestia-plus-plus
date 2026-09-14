#!/usr/bin/env bash
# One-time root setup for the caelestia "Dynamic" power mode.
#
# The GUI toggle (services/Dynamic.qml) only ever writes a plain state file it
# owns; everything here is what turns that file into an actual, continuously
# adapting power-plan. It runs as root purely for structural consistency with
# the sibling modes — the picker itself only calls powerprofilesctl, which a
# user session could do too, but keeping the same state-file + path-unit shape
# as max-perf/anti-heat/bed-mode is the point.
#
# Usage: sudo ./install.sh
set -euo pipefail

# Bump whenever ANY root-side file of this feature changes; the shell's
# system scan compares it against /etc/caelestia/dynamic.version and offers the
# upgrade automatically.
root_half_version=3

if [[ $EUID -ne 0 ]]; then
    echo "Run as root: sudo $0" >&2
    exit 1
fi

if ! command -v powerprofilesctl >/dev/null; then
    echo "powerprofilesctl not found. Install power-profiles-daemon first." >&2
    exit 1
fi

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

# The unit files and scripts are written against the dev machine's home;
# rewrite them for whoever is running the install.
user=${SUDO_USER:-}
[[ -z "$user" && -n "${PKEXEC_UID:-}" ]] && user=$(id -nu "$PKEXEC_UID")
[[ -z "$user" ]] && user=$(stat -c %U "$here")
if [[ -z $user || $user == root ]]; then
    echo "Could not determine the target user." >&2
    exit 1
fi
home=$(getent passwd "$user" | cut -d: -f6)
stage=$(mktemp -d)
for f in dynamic.service dynamic-sync.service dynamic-sync.path dynamic-apply dynamic-sync; do
    sed "s|/home/john|$home|g" "$here/$f" > "$stage/$f"
done

install -Dm644 "$stage/dynamic.service" /etc/systemd/system/dynamic.service
install -Dm644 "$stage/dynamic-sync.service" /etc/systemd/system/dynamic-sync.service
install -Dm644 "$stage/dynamic-sync.path" /etc/systemd/system/dynamic-sync.path
install -Dm755 "$stage/dynamic-apply" /usr/local/bin/dynamic-apply
install -Dm755 "$stage/dynamic-sync" /usr/local/bin/dynamic-sync

state_dir="$home/.local/state/caelestia"
state_file="$state_dir/dynamic"
install -d -o "$user" -g "$user" "$state_dir"
[[ -f "$state_file" ]] || printf '0\n' > "$state_file"
chown "$user:$user" "$state_file"

install -d /etc/caelestia
echo "$root_half_version" > /etc/caelestia/dynamic.version

systemctl daemon-reload
systemctl enable --now dynamic-sync.path
systemctl enable --now dynamic-sync.service

cat <<'EOF'

Setup complete.

Picking "Dynamic" in the battery menu (the fourth profile segment) now starts
the picker loop, which auto-switches between power-saver / balanced /
performance based on AC, battery %, and real CPU load. Check with:

  systemctl status dynamic.service
  journalctl -u dynamic.service -f      # watch the "dynamic -> <profile>" log
EOF
