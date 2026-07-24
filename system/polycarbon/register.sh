#!/usr/bin/env bash
# Desktop integration for Polycarbon, the Windows app runner. Run (idempotently) by the
# shell at startup: writes the handler .desktop entry and associates the
# Windows executable MIME types with it — but only types that have no
# default handler yet, so an existing Bottles/Lutris/wine setup is never
# hijacked. Prints one "claimed|<type>" / "kept|<type>|<handler>" line per
# MIME type so the system scan can report the state.
#
#   register.sh <shell-dir>           register handler + free MIME types
#   register.sh <shell-dir> --force   also take over already-claimed types
set -eu

SHELLDIR=${1:?shell dir}
FORCE=${2:-}
DESKTOP_ID="caelestia-polycarbon.desktop"
SETTINGS_ID="caelestia-polycarbon-settings.desktop"
LEGACY_ID="caelestia-winrun.desktop"
APPS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
MIME_TYPES="application/x-ms-dos-executable application/vnd.microsoft.portable-executable application/x-msdownload application/x-msi application/x-ms-shortcut text/x-msdos-batch text/vbscript text/x-ms-regedit"

mkdir -p "$APPS_DIR"
cat > "$APPS_DIR/$DESKTOP_ID" <<EOF
[Desktop Entry]
Type=Application
Name=Polycarbon Windows App
Comment=Runs Windows programs directly, no setup needed
Exec=$SHELLDIR/system/polycarbon/polycarbon %f
Icon=application-x-executable
Terminal=false
NoDisplay=true
MimeType=${MIME_TYPES// /;};
EOF

# Launcher-visible settings entry: opens the permission manager GUI
# (`polycarbon config`) from the app launcher.
cat > "$APPS_DIR/$SETTINGS_ID" <<EOF
[Desktop Entry]
Type=Application
Name=Polycarbon
GenericName=Windows App Settings
Comment=Permissions for Windows programs run through Polycarbon
Exec=$SHELLDIR/system/polycarbon/polycarbon config
Icon=preferences-system
Terminal=false
Categories=Settings;
EOF

for type in $MIME_TYPES; do
    current=$(xdg-mime query default "$type" 2>/dev/null || true)
    if [ -z "$current" ] || [ "$current" = "$DESKTOP_ID" ] || [ "$current" = "$LEGACY_ID" ] || [ "$FORCE" = "--force" ]; then
        xdg-mime default "$DESKTOP_ID" "$type"
        echo "claimed|$type"
    else
        echo "kept|$type|$current"
    fi
done

rm -f "$APPS_DIR/$LEGACY_ID"
command -v update-desktop-database >/dev/null && update-desktop-database "$APPS_DIR" || true

# Console entry point: `polycarbon config` (and `polycarbon foo.exe`)
mkdir -p "$HOME/.local/bin"
ln -sfn "$SHELLDIR/system/polycarbon/polycarbon" "$HOME/.local/bin/polycarbon"
# Drop the removed polyscrubber overlay's leftovers from older checkouts
rm -f "$HOME/.local/bin/polyscrubber"

# Put ~/.local/bin on PATH for the LOGIN shell (from getent, not $SHELL —
# $SHELL reflects whatever spawned this script, not the user's real login
# shell). The polycarbon/sandrunner commands live there; the system scan
# otherwise tells the user to edit their profile by hand.
# Idempotent: a marker line means we never append twice, and if the dir is
# already reachable (some other profile added it) nothing is written.
ensure_local_bin_path() {
    case ":$PATH:" in *":$HOME/.local/bin:"*) return 0 ;; esac
    local login_shell; login_shell=$(getent passwd "$(id -un)" 2>/dev/null | cut -d: -f7)
    local marker="# added by caelestia (polycarbon): ~/.local/bin on PATH"
    local rc line
    case "${login_shell##*/}" in
    fish)
        rc="$HOME/.config/fish/config.fish"
        line="fish_add_path -g \$HOME/.local/bin"
        ;;
    zsh)
        rc="${ZDOTDIR:-$HOME}/.zshrc"
        line='export PATH="$HOME/.local/bin:$PATH"'
        ;;
    bash)
        rc="$HOME/.bashrc"
        line='export PATH="$HOME/.local/bin:$PATH"'
        ;;
    *)
        # Unknown shell: fall back to ~/.profile, read by most POSIX logins
        rc="$HOME/.profile"
        line='export PATH="$HOME/.local/bin:$PATH"'
        ;;
    esac
    mkdir -p "$(dirname "$rc")"
    grep -qF "$marker" "$rc" 2>/dev/null && return 0
    printf '\n%s\n%s\n' "$marker" "$line" >> "$rc"
}
ensure_local_bin_path
