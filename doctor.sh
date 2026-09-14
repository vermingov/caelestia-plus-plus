#!/usr/bin/env bash
# Caelestia++ doctor: one-shot diagnosis of everything that has ever gone
# wrong on a foreign install. Run it, paste the whole output.
#   bash <(curl -fsSL https://raw.githubusercontent.com/vermingov/caelestia-plus-plus/main/doctor.sh)
set -u

pass() { printf 'PASS  %s\n' "$1"; }
fail() { printf 'FAIL  %s\n' "$1"; }
warn() { printf 'WARN  %s\n' "$1"; }

echo "=== Caelestia++ doctor $(date +%F\ %T) user=$USER host=$(hostname) ==="

echo "--- packages"
for p in caelestia++-shell caelestia++-cli caelestia++-quickshell; do
    if v=$(pacman -Q "$p" 2>/dev/null); then pass "$v"; else fail "$p not installed"; fi
done
if v=$(pacman -Q quickshell-git 2>/dev/null) && [[ $v == quickshell-git* ]]; then
    warn "$v still installed (pinned build not swapped in)"
fi

echo "--- shell checkout"
sd="$HOME/.config/quickshell/caelestia"
if [[ -d $sd/.git ]]; then
    pass "checkout at $(git -C "$sd" rev-parse --short HEAD) ($(git -C "$sd" log -1 --format=%s | cut -c1-60))"
    behind=$(git -C "$sd" rev-list --count HEAD..origin/main 2>/dev/null || echo "?")
    [[ $behind == 0 ]] && pass "up to date with origin/main" || warn "$behind commits behind origin/main — use Settings > Updates"
else
    fail "no git checkout at $sd"
fi

echo "--- stack"
pass "$(hyprctl version 2>/dev/null | head -1 || echo 'hyprctl unavailable')"
pass "quickshell binary: $(pacman -Qo /usr/bin/qs 2>/dev/null || echo 'unknown owner')"
pass "qt: $(pacman -Q qt6-base qt6-declarative qt6-wayland 2>/dev/null | tr '\n' ' ')"
# A prebuilt quickshell stops loading after a Qt patch release moves private
# symbols; the running shell survives on the old libraries, the next login does not
if timeout 20 qs --version >/dev/null 2>&1; then
    pass "qs binary starts: $(timeout 20 qs --version 2>/dev/null | head -1)"
else
    fail "qs binary does not start ($(timeout 20 qs --version 2>&1 | head -1 | cut -c1-120)) — fix: sudo bash $sd/system/quickshell/install.sh"
fi

echo "--- environment"
command -v powerprofilesctl >/dev/null 2>&1 && pass "power-profiles-daemon installed" \
    || warn "power-profiles-daemon missing — power profile toggles disabled, shell logs a dbus warning at start"
[[ -f $HOME/.face ]] && pass "avatar at ~/.face" \
    || warn "no ~/.face — dashboard avatar shows placeholder (click it in the dashboard to set one)"

echo "--- shell process"
if pgrep -f 'qs -c caelestia' >/dev/null; then
    pass "shell running"
else
    fail "shell not running — start with: caelestia shell -d"
fi

echo "--- monitors"
hyprctl monitors -j 2>/dev/null | python3 -c '
import json, sys
try:
    for m in json.load(sys.stdin):
        print("PASS  %s: %dx%d scale=%s pos=(%d,%d)" % (m["name"], m["width"], m["height"], m["scale"], m["x"], m["y"]))
except Exception as e:
    print("WARN  could not parse monitors:", e)'

echo "--- popouts per screen (hover the wifi/bluetooth icons on EACH monitor first for best data)"
for name in $(hyprctl monitors -j 2>/dev/null | python3 -c 'import json,sys; print(" ".join(m["name"] for m in json.load(sys.stdin)))'); do
    out=$(qs -c caelestia ipc call "popouts-$name" state 2>&1)
    case $out in
        '{'*) pass "popouts-$name: $out" ;;
        *)    fail "popouts-$name: $out" ;;
    esac
done

echo "--- hyprland layer rules touching the shell"
found=0
while IFS= read -r line; do
    found=1
    case $line in
        *ignorealpha*|*ignorezero*) fail "input-transparent rule: $line" ;;
        *) warn "layerrule present: $line" ;;
    esac
done < <(grep -rn "layerrule" "$HOME/.config/hypr/" 2>/dev/null | grep -iE "caelestia|drawers|launcher|bar|\*" || true)
[[ $found == 0 ]] && pass "no layer rules targeting shell namespaces"

echo "--- easter egg"
if id -nG "$USER" | grep -qw input; then pass "user in input group"; else fail "user NOT in input group — rerun installer, then re-login"; fi
readable=0
for f in /dev/input/event*; do [[ -r $f ]] && readable=1 && break; done
if [[ $readable == 1 ]]; then
    pass "/dev/input readable in THIS session"
else
    if id -nG "$USER" | grep -qw input; then
        fail "/dev/input NOT readable yet — rerun the installer (it grants this session access via setfacl)"
    else
        fail "/dev/input not readable"
    fi
fi
if pgrep -f "penis-egg-watch" >/dev/null; then pass "egg watcher running"; else fail "egg watcher not running (shell spawns it at startup — restart the shell)"; fi
command -v python3 >/dev/null && pass "python3 present" || fail "python3 missing"
if qs -c caelestia ipc show 2>/dev/null | grep -q "target easterEgg"; then
    pass "easterEgg IPC target registered"
else
    fail "easterEgg IPC target missing from the running shell"
fi
[[ -f $HOME/.config/quickshell/caelestia/assets/penis-egg-watch.py ]] \
    && pass "watcher script present in checkout" || fail "watcher script missing from checkout"
if [[ ${1:-} == --pop ]]; then
    echo ":: popping the egg via IPC (watch the bottom of your screen)"
    qs -c caelestia ipc call easterEgg pop
fi

echo "--- recent shell log (errors/warnings after your hovers land here)"
logdir=$(ls -td /run/user/*/quickshell/by-id/*/ 2>/dev/null | head -1)
if [[ -n $logdir && -f $logdir/log.log ]]; then
    grep -iE "error|warn" "$logdir/log.log" 2>/dev/null \
        | grep -viE "\.face|Tokens\.padding|dbus|upower|StatusNotifier|desktopentry" \
        | tail -25
    echo "(benign .face/Tokens/dbus warnings filtered)"
else
    warn "no quickshell log dir found"
fi

echo "--- extras"
[[ -f $HOME/.config/fastfetch/config.jsonc ]] && grep -q 'Caelestia++' "$HOME/.config/fastfetch/config.jsonc" \
    && pass "Caelestia++ fastfetch config installed" || warn "Caelestia++ fastfetch config not installed (rerun installer)"
grep -q 'IgnorePkg.*caelestia++' /etc/pacman.conf 2>/dev/null && pass "IgnorePkg set" || warn "IgnorePkg not set in /etc/pacman.conf"

echo "=== end ==="
