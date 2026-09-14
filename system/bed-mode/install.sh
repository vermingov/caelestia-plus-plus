#!/usr/bin/env bash
# One-time root setup for the caelestia "bed-mode" fan-curve toggle.
#
# The GUI toggle (services/BedMode.qml) only ever writes a plain state file
# it owns; everything here is what turns that file into an actual fan-curve
# change, running as root so the shell process never needs elevated rights.
#
# Usage: sudo ./install.sh
set -euo pipefail

# Bump whenever ANY root-side file of this feature changes; the shell's
# system scan compares it against /etc/caelestia/bed-mode.version and offers the
# upgrade automatically.
root_half_version=4

if [[ $EUID -ne 0 ]]; then
    echo "Run as root: sudo $0" >&2
    exit 1
fi

if ! command -v thinkfan >/dev/null; then
    echo "thinkfan not found. Install it first (AUR): paru -S thinkfan" >&2
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

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
for f in bed-mode-sync bed-mode-sync.path; do
    sed "s|/home/john|$target_home|g" "$here/$f" > "$stage/$f"
done

install -Dm644 "$here/thinkfan-fan-control.conf" /etc/modprobe.d/thinkfan-fan-control.conf
install -Dm644 "$here/thinkfan-bed.yaml" /etc/thinkfan-bed.yaml
install -Dm644 "$here/thinkfan-bed.service" /etc/systemd/system/thinkfan-bed.service
install -Dm644 "$here/bed-mode-sync.service" /etc/systemd/system/bed-mode-sync.service
install -Dm644 "$stage/bed-mode-sync.path" /etc/systemd/system/bed-mode-sync.path
install -Dm755 "$stage/bed-mode-sync" /usr/local/bin/bed-mode-sync

state_dir=$target_home/.local/state/caelestia
state_file="$state_dir/bed-mode"
install -d -o "$target_user" -g "$target_user" "$state_dir"
[[ -f "$state_file" ]] || printf '0\n' > "$state_file"
chown "$target_user:$target_user" "$state_file"

install -d /etc/caelestia
echo "$root_half_version" > /etc/caelestia/bed-mode.version

systemctl daemon-reload
systemctl enable --now bed-mode-sync.path
systemctl enable --now bed-mode-sync.service

cat <<'EOF'

Setup complete.

thinkpad_acpi still needs to be reloaded with fan_control=1 for manual fan
control to actually work. Either reboot now, or reload it live:

  sudo modprobe -r thinkpad_acpi && sudo modprobe thinkpad_acpi

(A live reload can briefly drop the fan to firmware defaults; rebooting is
the safer option.)

After that, flipping "Bed mode" in the battery popout will start/stop
thinkfan-bed.service automatically. Check it with:

  systemctl status thinkfan-bed.service
  journalctl -u thinkfan-bed.service -f
EOF
