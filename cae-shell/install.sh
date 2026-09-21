#!/usr/bin/env bash
# Build and install cae: the shell that draws the bar, its popouts, the
# launcher and the notifications, in one process with no web engine in it.
#
# No root: the binary goes in ~/.local/bin. Quickshell runs it in place of the
# Tauri bar wherever it is installed, and goes back to that bar (or to its own
# QML one) where it is not.
#
# Usage: ./install.sh              install (builds first)
#        ./install.sh --standalone install, and have the graphical session
#                                  start it instead of Quickshell
#        ./install.sh --uninstall  remove
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
bindir="${XDG_BIN_HOME:-$HOME/.local/bin}"
target="$bindir/cae-shell"
# The bar this one takes over from. Left installed, and left running: it is
# what Quickshell falls back to where cae is not installed, and which of the
# two Quickshell runs is decided when Quickshell starts.
old_bar="$bindir/caelestia-bar"

# Says that the installed bar is one that serves notifications. Quickshell's
# own server only stands down for a bar that has said so.
state="${XDG_STATE_HOME:-$HOME/.local/state}/caelestia"
serves_notifs="$state/bar-serves-notifs"
# The same for the other pieces this draws, a file each: while one says so,
# Quickshell's own stays down and its keys are passed along to this one.
pieces=(dashboard session osd background utilities idle picker guard security features lock battery scan egg cinema)

# Ends a running cae, so that the one Quickshell starts in its place is the
# one just installed. Quickshell starts whatever exits again.
#
# By executable rather than by name, and knowing a replaced one by its
# "(deleted)": a running shell keeps the file it was started from. Never the
# Tauri bar: a Quickshell that is running that one would only start it again,
# and the screen would have blinked for nothing.
stop_running() {
    # Once the session is cae's own, systemd owns the process: killing it
    # from outside is a race against `Restart=`, and losing that race is a
    # desktop with nothing on it. Ask systemd instead — it stops the old one
    # and starts the new one, in that order, with no gap to fall into.
    if systemctl --user is-active --quiet cae-shell.service 2>/dev/null; then
        systemctl --user restart cae-shell.service
        return
    fi
    local pid exe
    for pid in /proc/[0-9]*; do
        pid=${pid#/proc/}
        exe=$(readlink "/proc/$pid/exe" 2>/dev/null) || continue
        case "$exe" in
            "$target"|"$target (deleted)") kill "$pid" 2>/dev/null || true ;;
        esac
    done
}

# Whether the shell that is running would start cae, which is whether it is
# already running one.
handed_over() {
    local pid
    for pid in /proc/[0-9]*; do
        [[ $(readlink "$pid/exe" 2>/dev/null) == "$target"* ]] && return 0
    done
    return 1
}

units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
service="$units/cae-shell.service"

# Starts cae with the graphical session, which is what takes Quickshell's
# last job away. Only ever on request: while Quickshell is the one starting
# it, a second one would find the lock taken and exit.
stand_alone() {
    mkdir -p "$units"
    install -m644 "$here/cae-shell.service" "$service"
    systemctl --user daemon-reload
    systemctl --user enable cae-shell.service
    echo
    echo "cae will start with the graphical session."
    echo "Take Quickshell out of it by removing this line from"
    echo "~/.config/hypr/hyprland/execs.lua:"
    echo '    hl.exec_cmd("caelestia shell -d")'
    echo "and starting cae now with: systemctl --user start cae-shell"
}

if [[ ${1:-} == --uninstall ]]; then
    rm -f "$target"
    for piece in "${pieces[@]}"; do rm -f "$state/bar-serves-$piece"; done
    if [[ -f $service ]]; then
        systemctl --user disable --now cae-shell.service 2>/dev/null || true
        rm -f "$service"
        systemctl --user daemon-reload
    fi
    # The unit is gone by now, so this stops whatever is left by hand.
    stop_running
    echo "Removed $target"
    if [[ -x $old_bar ]]; then
        echo "The Tauri bar is back in charge, once the shell has been restarted: cae restart"
    else
        rm -f "$serves_notifs"
        echo "The shell's own bar, launcher and notifications are back in charge, once it has been restarted: cae restart"
    fi
    exit 0
fi

command -v cargo >/dev/null || { echo "cargo not found: install rust first" >&2; exit 1; }

# The copy of GPUI with the lock screen's window kind in it. A no-op once it
# is there.
"$here/vendor.sh"

echo ">> Building cae-shell"
cargo build --release --manifest-path "$here/Cargo.toml" --bin cae-shell
built="$here/target/release/cae-shell"

# Nothing to do, and so nothing to restart: a build that changed nothing is
# what most runs of this are.
if [[ -x $target ]] && cmp -s "$built" "$target"; then
    echo "cae-shell is already current"
    [[ ${1:-} == --standalone ]] && stand_alone
    exit 0
fi

mkdir -p "$bindir" "$state"
install -m755 "$built" "$target.new"
mv -f "$target.new" "$target"
: > "$serves_notifs"
for piece in "${pieces[@]}"; do : > "$state/bar-serves-$piece"; done

was_running=false
handed_over && was_running=true

# Stopped only once the new binary is where the old one was. Quickshell starts
# the bar again six tenths of a second after it exits, and one stopped first
# would be started again from the file that was about to be replaced.
stop_running

if [[ ${1:-} == --standalone ]]; then
    stand_alone
fi

echo
echo "Installed $target"
if systemctl --user is-active --quiet cae-shell.service 2>/dev/null; then
    echo "  (restarted through cae-shell.service)"
elif ! pgrep -x qs >/dev/null 2>&1; then
    echo "  (the shell is not running: it will start cae when it does)"
elif ! $was_running; then
    # Which bar Quickshell runs is decided when it starts, and the one that
    # is running decided before this was installed.
    echo "  the running shell is still on its old bar: cae restart hands over"
fi
echo "  undo: $here/install.sh --uninstall"
