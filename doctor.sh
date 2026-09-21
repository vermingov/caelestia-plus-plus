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

# Which shell this machine runs. cae draws the desktop by itself now; a
# machine that has not been handed over yet still has Quickshell starting it,
# and nearly everything below is asked differently of the two.
cae_unit=cae-shell.service
standalone() { systemctl --user is-enabled --quiet "$cae_unit" 2>/dev/null; }

echo "--- stack"
pass "$(hyprctl version 2>/dev/null | head -1 || echo 'hyprctl unavailable')"
if standalone; then
    pass "the session is cae's own ($cae_unit); Quickshell is not in it"
else
    pass "quickshell binary: $(pacman -Qo /usr/bin/qs 2>/dev/null || echo 'unknown owner')"
    pass "qt: $(pacman -Q qt6-base qt6-declarative qt6-wayland 2>/dev/null | tr '\n' ' ')"
    # A prebuilt quickshell stops loading after a Qt patch release moves private
    # symbols; the running shell survives on the old libraries, the next login does not
    if timeout 20 qs --version >/dev/null 2>&1; then
        pass "qs binary starts: $(timeout 20 qs --version 2>/dev/null | head -1)"
    else
        fail "qs binary does not start ($(timeout 20 qs --version 2>&1 | head -1 | cut -c1-120)) — fix: sudo bash $sd/system/quickshell/install.sh"
    fi
    # Told not to start Quickshell, with no unit to start anything else: the
    # session has nothing in it and will not say so until the next login.
    if grep -qE '^[[:space:]]*-- cae starts from cae-shell\.service now' "$HOME/.config/hypr/hyprland/execs.lua" 2>/dev/null; then
        fail "the session starts NOTHING: Quickshell is commented out of execs.lua and cae-shell.service is not enabled — the next login comes up empty. Fix: cae migrate, or python3 $sd/cae-shell/standalone.py --undo to put Quickshell back"
    else
        warn "Quickshell still starts the shell — hand it over with: cae migrate"
    fi
fi

echo "--- environment"
command -v powerprofilesctl >/dev/null 2>&1 && pass "power-profiles-daemon installed" \
    || warn "power-profiles-daemon missing — power profile toggles disabled, shell logs a dbus warning at start"
[[ -f $HOME/.face ]] && pass "avatar at ~/.face" \
    || warn "no ~/.face — dashboard avatar shows placeholder (click it in the dashboard to set one)"

echo "--- shell process"
if standalone; then
    if systemctl --user is-active --quiet "$cae_unit"; then
        pass "cae running ($(systemctl --user show -p MainPID --value "$cae_unit"))"
    else
        fail "cae not running — start with: systemctl --user start $cae_unit"
    fi
    if hyprctl layers 2>/dev/null | grep -q 'caelestia-bar'; then
        pass "a bar is on the screen"
    else
        fail "nothing is drawing a bar — see: journalctl --user -u $cae_unit"
    fi
    # A window that cannot be given a drawing surface is logged and stepped
    # over, so the shell stays up with pieces of it simply missing. Worth
    # saying plainly, with the one setting that chooses a different GPU.
    if journalctl --user -u "$cae_unit" -n 400 --no-pager 2>/dev/null \
        | grep -q 'is not compatible with the display surface'; then
        fail "the GPU cae chose cannot draw some of its windows, so they never opened"
        journalctl --user -u "$cae_unit" -n 400 --no-pager 2>/dev/null \
            | grep -o 'Adapter "[^"]*" (backend=[^)]*)' | sort -u | sed 's/^/        /'
        echo "        the machine's GPUs:"
        lspci -nn 2>/dev/null | grep -iE 'vga|3d controller' | sed 's/^/          /'
        echo "        pick another with its [vendor:device] id, second half, e.g.:"
        echo "          mkdir -p ~/.config/caelestia"
        echo "          echo ZED_DEVICE_ID=0x1b81 > ~/.config/caelestia/cae-shell.env"
        echo "          systemctl --user restart $cae_unit"
    fi
elif pgrep -f 'qs -c caelestia' >/dev/null; then
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

if standalone; then
    # cae keeps no popout state to ask for; what it draws is on the screen,
    # and the screen is what the compositor will happily list.
    echo "--- surfaces"
    hyprctl layers 2>/dev/null | grep -oE 'namespace: caelestia-[a-z-]+' | sort | uniq -c \
        | while read -r count namespace; do pass "${namespace#namespace: } x$count"; done
else
    echo "--- popouts per screen (hover the wifi/bluetooth icons on EACH monitor first for best data)"
    for name in $(hyprctl monitors -j 2>/dev/null | python3 -c 'import json,sys; print(" ".join(m["name"] for m in json.load(sys.stdin)))'); do
        out=$(qs -c caelestia ipc call "popouts-$name" state 2>&1)
        case $out in
            '{'*) pass "popouts-$name: $out" ;;
            *)    fail "popouts-$name: $out" ;;
        esac
    done
fi

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
if pgrep -f "penis-egg-watch" >/dev/null || pgrep -f "egg-watch" >/dev/null; then pass "egg watcher running"; else fail "egg watcher not running (the shell starts it — restart the shell)"; fi
command -v python3 >/dev/null && pass "python3 present" || fail "python3 missing"
if standalone; then
    # cae answers at its door rather than over Quickshell's IPC. The door
    # being there is the whole of the question: it is the running shell that
    # opens it, and every verb goes through it.
    if [[ -S ${XDG_RUNTIME_DIR:-/tmp}/caelestia-shell.sock ]]; then
        pass "cae's door is open (egg reachable as: cae-shell egg)"
    else
        fail "cae's door is not there — the shell is not running"
    fi
elif qs -c caelestia ipc show 2>/dev/null | grep -q "target easterEgg"; then
    pass "easterEgg IPC target registered"
else
    fail "easterEgg IPC target missing from the running shell"
fi
[[ -f $HOME/.config/quickshell/caelestia/assets/penis-egg-watch.py ]] \
    && pass "watcher script present in checkout" || fail "watcher script missing from checkout"
if [[ ${1:-} == --pop ]]; then
    echo ":: popping the egg (watch the bottom of your screen)"
    if standalone; then cae-shell egg; else qs -c caelestia ipc call easterEgg pop; fi
fi

echo "--- recent shell log (errors/warnings after your hovers land here)"
if standalone; then
    journalctl --user -u "$cae_unit" -n 200 --no-pager 2>/dev/null \
        | grep -iE "error|warn" | grep -viE "dbus|upower|StatusNotifier" | tail -25
    echo "(from journalctl --user -u $cae_unit)"
    exit 0
fi
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
