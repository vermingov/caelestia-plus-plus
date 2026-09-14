#!/usr/bin/env bash
# Caelestia++ one-command installer for Arch.
# Usage: bash <(curl -fsSL https://raw.githubusercontent.com/vermingov/caelestia-plus-plus/main/install.sh)
set -euo pipefail

REPO=vermingov/caelestia-plus-plus
# First home of the repo; checkouts still pointing there get repointed
OLD_REPO=basement-interactive/caelestia-plus-plus
SHELL_DIR="$HOME/.config/quickshell/caelestia"

[[ $EUID -eq 0 ]] && { echo "run as your normal user, not root"; exit 1; }
command -v pacman >/dev/null || { echo "this installer is Arch-only"; exit 1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo ":: downloading Caelestia++ packages from the latest release"
release_json=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest")
{ grep -o 'https://[^"]*\.pkg\.tar\.zst' <<<"$release_json" || true; } | sort -u > "$tmp/urls"
[[ -s $tmp/urls ]] || { echo "no package assets on the latest release"; exit 1; }
while read -r url; do
    # GitHub percent-encodes '+' in asset URLs (case-insensitively)
    fname=$(basename "$url" | sed 's/%2[Bb]/+/g')
    curl -fL -o "$tmp/$fname" "$url"
done < "$tmp/urls"

deps=$(for pkg in "$tmp"/*.pkg.tar.zst; do
    bsdtar -xOf "$pkg" .PKGINFO | awk -F' = ' '$1 == "depend" {print $2}'
done | { grep -v '^caelestia' || true; } | sort -u)

# pacman -T reports which of these aren't satisfied yet (respects provides);
# only then is an AUR helper needed at all
# shellcheck disable=SC2086 — deps is a word list by construction
missing=$(pacman -T $deps || true)

if [[ -z $missing ]]; then
    echo ":: all dependencies already installed"
else
    echo ":: missing dependencies:" $missing
    if command -v paru >/dev/null; then
        aur=paru
    elif command -v yay >/dev/null; then
        aur=yay
    else
        echo ":: no AUR helper found, installing paru"
        sudo pacman -S --needed --noconfirm base-devel git
        git clone https://aur.archlinux.org/paru-bin.git "$tmp/paru-bin"
        (cd "$tmp/paru-bin" && makepkg -si --noconfirm)
        aur=paru
    fi
    # shellcheck disable=SC2086
    $aur -S --needed --asdeps --noconfirm $missing
fi

# Old caelestia packages our replacements don't declare conflicts against
# (plain caelestia-shell/meta) must go first; the window between removal and
# install is guarded so an abort tells the user how to recover. quickshell's
# swap needs no removal — the ++ package declares the conflict and --ask
# resolves it inside one atomic, rollback-safe transaction.
old_pkgs=$(pacman -Qq 2>/dev/null | grep -E '^caelestia-(shell|shell-git|cli|meta)$' || true)
if [[ -n $old_pkgs ]]; then
    echo ":: replacing regular caelestia packages:" $old_pkgs
    trap 'echo "!! aborted mid-swap: caelestia packages were removed but not yet replaced."
          echo "!! rerun this installer to finish (dependencies are fine; skip orphan cleanup until then)."' ERR
    # shellcheck disable=SC2086
    sudo pacman -Rdd --noconfirm $old_pkgs
fi

echo ":: installing Caelestia++ packages"
# --ask=22 auto-answers conflict/replace removals inside the -U transaction
sudo pacman -U --noconfirm --ask=22 "$tmp"/*.pkg.tar.zst
trap 'rm -rf "$tmp"' EXIT ERR

if ! grep -q '^IgnorePkg.*caelestia++' /etc/pacman.conf; then
    sudo sed -i '/^\[options\]/a IgnorePkg = caelestia++-shell caelestia++-cli caelestia++-quickshell' /etc/pacman.conf
    grep -q '^IgnorePkg.*caelestia++' /etc/pacman.conf \
        || echo "WARN: could not add IgnorePkg (no [options] section?) — add it to /etc/pacman.conf manually"
fi

# The shell's easter-egg watcher reads keyboard evdev devices for its
# 5-letter trigger (keeps only the last 5 letter keycodes in RAM, logs
# nothing). Group membership makes it permanent; the ACL grant makes it
# work in THIS session without re-logging (the watcher rescans within 60s).
if ! id -nG "$USER" | grep -qw input; then
    echo ":: adding $USER to the 'input' group for the easter-egg watcher"
    echo "   (remove anytime: sudo gpasswd -d $USER input)"
    sudo usermod -aG input "$USER"
fi
readable=0
for f in /dev/input/event*; do [[ -r $f ]] && readable=1 && break; done
if [[ $readable == 0 ]]; then
    echo ":: granting the current session read access to input devices"
    sudo setfacl -m "u:$USER:r" /dev/input/event* 2>/dev/null || true
fi
# A one-shot ACL dies when a device re-enumerates (unplug/replug); the
# uaccess tag has logind maintain the ACL for the active seat user on every
# input node, current and future.
if [[ ! -f /etc/udev/rules.d/70-caelestia-egg.rules ]]; then
    echo ":: installing udev rule so input access survives hotplug and re-login"
    echo 'SUBSYSTEM=="input", KERNEL=="event*", TAG+="uaccess"' | sudo tee /etc/udev/rules.d/70-caelestia-egg.rules >/dev/null
    sudo udevadm control --reload-rules
    sudo udevadm trigger --subsystem-match=input
fi

echo ":: fetching the shell"
if [[ -d $SHELL_DIR/.git ]] && git -C "$SHELL_DIR" remote get-url origin 2>/dev/null | grep -q "$REPO"; then
    echo "   Caelestia++ checkout found, updating"
    git -C "$SHELL_DIR" pull --ff-only || echo "   pull failed (local changes?) — leaving checkout as is"
elif [[ -d $SHELL_DIR/.git ]] && git -C "$SHELL_DIR" remote get-url origin 2>/dev/null | grep -q "$OLD_REPO"; then
    echo "   Caelestia++ checkout from the previous home found, repointing to $REPO"
    git -C "$SHELL_DIR" remote set-url origin "https://github.com/$REPO.git"
    git -C "$SHELL_DIR" pull --ff-only || echo "   pull failed (local changes?) — leaving checkout as is"
elif [[ -e $SHELL_DIR ]]; then
    # Clone to scratch first so a network failure displaces nothing
    git clone "https://github.com/$REPO.git" "$tmp/shell-clone"
    backup="$SHELL_DIR.backup-$(date +%Y%m%d-%H%M%S)"
    echo "   existing non-Caelestia++ shell config found, moving to $backup"
    mv "$SHELL_DIR" "$backup"
    mv "$tmp/shell-clone" "$SHELL_DIR"
else
    git clone "https://github.com/$REPO.git" "$SHELL_DIR"
fi

# A prebuilt quickshell only starts against the Qt it was built with; Qt patch
# releases move private symbols. The root half builds it here when the shipped
# one does not load, and keeps the scheduler from filing the shell as a
# background service either way.
echo ":: checking that quickshell starts on this Qt"
sudo bash "$SHELL_DIR/system/quickshell/install.sh"

# sandrunner (fake-root simulation sandbox) is a user-level script: a PATH
# symlink is the whole install, and it tracks the checkout across updates.
# bubblewrap runs the sandbox; fuse-overlayfs provides the writable
# throwaway system view (without it the sandbox falls back to read-only).
if [[ -f $SHELL_DIR/system/sandrunner/sandrunner ]]; then
    mkdir -p "$HOME/.local/bin"
    ln -sf "$SHELL_DIR/system/sandrunner/sandrunner" "$HOME/.local/bin/sandrunner"
    if ! command -v bwrap >/dev/null || ! command -v fuse-overlayfs >/dev/null; then
        sudo pacman -S --needed --noconfirm bubblewrap fuse-overlayfs
    fi
fi

# `cae` is the update command: one invocation pulls the shell, installs newer
# release packages and privileged halves, and restarts. A PATH symlink into
# the checkout is the whole install, so it updates itself with the repo.
mkdir -p "$HOME/.local/bin"
ln -sf "$SHELL_DIR/cae" "$HOME/.local/bin/cae"
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) echo "WARN: $HOME/.local/bin is not on your PATH — 'cae' will not be found until it is" ;;
esac

# hallucinate (AI-dreamed one-shot apps) is a user-level script too; Tkinter
# (the tk package) is its only extra dependency.
if [[ -f $SHELL_DIR/system/hallucinate/hallucinate ]]; then
    mkdir -p "$HOME/.local/bin"
    ln -sf "$SHELL_DIR/system/hallucinate/hallucinate" "$HOME/.local/bin/hallucinate"
    python3 -c "import tkinter" >/dev/null 2>&1 || sudo pacman -S --needed --noconfirm tk
fi

# Caelestia++ ships a full-info fastfetch config (DE row says Caelestia++).
# Verify the source exists (stale checkout) and copy — never move — the
# user's config aside so an abort can't lose it.
if [[ -f $SHELL_DIR/assets/fastfetch.jsonc ]]; then
    ff_conf="$HOME/.config/fastfetch/config.jsonc"
    if [[ -f $ff_conf ]] && ! grep -q 'Caelestia++' "$ff_conf"; then
        echo ":: existing fastfetch config backed up to config.jsonc.pre-caelestia++"
        cp "$ff_conf" "$ff_conf.pre-caelestia++"
    fi
    install -Dm644 "$SHELL_DIR/assets/fastfetch.jsonc" "$ff_conf"
else
    echo "WARN: shell checkout has no fastfetch asset — skipped"
fi

# Without an autostart the shell is gone after the next login
if ! grep -rqs "caelestia shell" "$HOME/.config/hypr/" 2>/dev/null; then
    if [[ -f $HOME/.config/hypr/hyprland.conf ]]; then
        echo ":: adding Hyprland autostart (exec-once = caelestia shell -d)"
        printf '\nexec-once = caelestia shell -d\n' >> "$HOME/.config/hypr/hyprland.conf"
    else
        echo "WARN: no Hyprland config found — add an autostart for: caelestia shell -d"
    fi
fi

echo
if pgrep -f 'qs -c caelestia' >/dev/null 2>&1; then
    echo ":: a shell instance is already running — restarting it as Caelestia++"
    qs -c caelestia kill 2>/dev/null || pkill -f 'qs -c caelestia' || true
    sleep 1
fi
if [[ -n ${WAYLAND_DISPLAY:-}${HYPRLAND_INSTANCE_SIGNATURE:-} ]]; then
    setsid -f caelestia shell -d >/dev/null 2>&1 || true
    sleep 2
    if pgrep -f 'qs -c caelestia' >/dev/null 2>&1; then
        echo "Caelestia++ installed and running."
    else
        echo "Caelestia++ installed, but the shell did not come up — start it manually: caelestia shell -d"
    fi
else
    echo "Caelestia++ installed. Not inside a graphical session — start it from Hyprland: caelestia shell -d"
fi
echo "Updates arrive in the shell's Settings > Updates tab."
