#!/usr/bin/env bash
# Probe half of the system scan (services/SystemCheck.qml). Runs unprivileged,
# prints one machine-readable "key|field|field" line per check and never
# blocks: anything that can stall is timeout-wrapped, anything optional is
# guarded. Values must not contain "|" (tr'd away where user data flows in).
#
#   $1  shell checkout directory
#   $2  wallpapers directory (resolved by the shell's config)
#   $3  space-separated list of runtime binaries to probe
set -u

SHELLDIR=${1:?shell dir}
WALLSDIR=${2:-}
BINS=${3:-}

# --- Shell runtime -----------------------------------------------------------
for b in $BINS; do
    command -v "$b" >/dev/null 2>&1 && echo "bin|$b|ok" || echo "bin|$b|missing"
done

systemctl is-active -q power-profiles-daemon 2>/dev/null && echo "ppd|active" || echo "ppd|inactive"
echo "pmconflict|$(pacman -Qq tlp auto-cpufreq laptop-mode-tools tuned tuned-ppd 2>/dev/null | tr '\n' ' ')"
echo "ppdunit|$(systemctl is-enabled power-profiles-daemon 2>&1 | head -1 | cut -c1-40 | tr '|' '/')|$(systemctl is-active power-profiles-daemon 2>&1 | head -1 | cut -c1-20)"
[ -f "$HOME/.face" ] && echo "face|ok" || echo "face|missing"
grep -qs 'IgnorePkg.*caelestia' /etc/pacman.conf && echo "ignpkg|ok" || echo "ignpkg|missing"
timeout 5 caelestia --version >/dev/null 2>&1 && echo "cli|ok" || echo "cli|broken"
[ -n "$WALLSDIR" ] && { [ -d "$WALLSDIR" ] && echo "walldir|ok|$WALLSDIR" || echo "walldir|missing|$WALLSDIR"; }

for f in max-perf anti-heat dynamic bed-mode; do
    repo=$(grep -m1 '^root_half_version=' "$SHELLDIR/system/$f/install.sh" 2>/dev/null | cut -d= -f2)
    [ -n "$repo" ] || repo=0
    inst=$(cat "/etc/caelestia/$f.version" 2>/dev/null || echo 0)
    if systemctl is-enabled -q "$f-sync.path" 2>/dev/null; then en=1; else en=0; fi
    echo "ver|$f|$repo|$inst|$en"
done
# redguard and redwall are plain services (no sync.path); track them the same way
for f in redguard redwall; do
    repo=$(grep -m1 '^root_half_version=' "$SHELLDIR/system/$f/install.sh" 2>/dev/null | cut -d= -f2)
    [ -n "$repo" ] || repo=0
    inst=$(cat "/etc/caelestia/$f.version" 2>/dev/null || echo 0)
    if systemctl is-enabled -q "${f}d.service" 2>/dev/null; then en=1; else en=0; fi
    echo "ver|$f|$repo|$inst|$en"
done
echo "gitdirty|$(git -C "$SHELLDIR" status --porcelain 2>/dev/null | grep -c .)"

# sandrunner is user-level (no root half): a symlink in ~/.local/bin is the
# whole install (its packages are probed via the binaries list). Also report
# whether ~/.local/bin is on PATH.
sr_link=missing
[ "$(readlink -f "$HOME/.local/bin/sandrunner" 2>/dev/null)" = "$(readlink -f "$SHELLDIR/system/sandrunner/sandrunner")" ] && sr_link=ok
case ":$PATH:" in *":$HOME/.local/bin:"*) sr_path=ok ;; *) sr_path=missing ;; esac
echo "sandrunner|$sr_link|$sr_path"

# hallucinate: same user-level symlink model. Tk (python-tkinter native lib)
# is its only extra dependency; probe it by importing tkinter.
hl_link=missing
[ "$(readlink -f "$HOME/.local/bin/hallucinate" 2>/dev/null)" = "$(readlink -f "$SHELLDIR/system/hallucinate/hallucinate")" ] && hl_link=ok
python3 -c "import tkinter" >/dev/null 2>&1 && hl_tk=ok || hl_tk=missing
echo "hallucinate|$hl_link|$hl_tk"

# Bar entry ids from the user config, for typo detection shell-side
for c in "$HOME/.config/caelestia/"*.json; do
    [ -e "$c" ] || continue
    python3 -m json.tool "$c" >/dev/null 2>&1 && echo "cfgjson|$c|ok" || echo "cfgjson|$c|bad"
done

# --- Notification daemon ownership ---------------------------------------------
# Whoever owns org.freedesktop.Notifications on the session bus renders every
# notification. If that is not this quickshell, the shell's own (much nicer)
# notifications never appear — a stray dunst/mako/swaync grabbed the name
# first. Report the owning process and, if it is a systemd user unit, which
# one, so the scan can offer to stop it and hand notifications back.
nowner=$(busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus GetNameOwner s org.freedesktop.Notifications 2>/dev/null | awk '{print $2}' | tr -d '"')
ncomm=""; nunit=""
if [ -n "$nowner" ]; then
    npid=$(busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus GetConnectionUnixProcessID s "$nowner" 2>/dev/null | awk '{print $2}')
    [ -n "$npid" ] && ncomm=$(tr -d '\0' < "/proc/$npid/comm" 2>/dev/null)
    # Only a dedicated daemon unit is safe to mask. A daemon started from the
    # compositor's exec-once shares the compositor's own cgroup unit
    # (wayland-wm@…, graphical-session, the session scope) — masking that
    # would kill the desktop, so those are never treated as the culprit unit.
    [ -n "$npid" ] && nunit=$(grep -oE '[a-zA-Z0-9@._-]+\.service' "/proc/$npid/cgroup" 2>/dev/null | grep -viE 'user@|init\.scope|wayland-wm|graphical-session|hyprland|plasma|gnome-session|session\.slice' | head -1)
fi
echo "notifowner|${ncomm:-none}|${nunit}"

# --- Hyprland ----------------------------------------------------------------
hc=$(timeout 5 hyprctl configerrors 2>/dev/null | grep -vi 'no errors' | grep .) || true
echo "hyprerr|$(printf '%s' "$hc" | grep -c .)|$(printf '%s' "$hc" | head -1 | cut -c1-140 | tr '|' '/')"
for p in xdg-desktop-portal-hyprland qt6-wayland; do
    pacman -Q "$p" >/dev/null 2>&1 && echo "pkg|$p|ok" || echo "pkg|$p|missing"
done
echo "portals|$(pacman -Qq 2>/dev/null | grep '^xdg-desktop-portal-' | grep -v hyprland | grep -v gtk | tr '\n' ' ')"
systemctl --user is-active -q xdg-desktop-portal 2>/dev/null && echo "portalsvc|active" || echo "portalsvc|inactive"
pgrep -f 'polkit-gnome-auth|polkit-kde-auth|lxpolkit|hyprpolkitagent|mate-polkit|xfce-polkit' >/dev/null && echo "polkitagent|ok" || echo "polkitagent|missing"

# Monitors running below their panel's best refresh rate (same resolution)
timeout 5 hyprctl monitors -j 2>/dev/null | python3 - <<'PY' 2>/dev/null || echo "refresh|0|"
import json, sys
try:
    low = []
    for m in json.load(sys.stdin):
        cur = m.get("refreshRate", 0)
        res = "%dx%d" % (m.get("width", 0), m.get("height", 0))
        best = cur
        for mode in m.get("availableModes", []):
            if mode.startswith(res + "@"):
                try:
                    best = max(best, float(mode.split("@")[1].replace("Hz", "")))
                except ValueError:
                    pass
        if best - cur > 1:
            low.append("%s %.0fHz (max %.0fHz)" % (m.get("name", "?"), cur, best))
    print("refresh|%d|%s" % (len(low), ", ".join(low)))
except Exception:
    print("refresh|0|")
PY

# --- Desktop entries ---------------------------------------------------------
# Broken Exec (binary gone) and structurally invalid files. NoDisplay and
# OnlyShowIn entries are skipped: they are not launchable apps here anyway.
nb=0; bl=""; nbu=0; bu=""; nm=0; ml=""; mu=""
for d in /usr/share/applications "$HOME/.local/share/applications"; do
    [ -d "$d" ] || continue
    for f in "$d"/*.desktop; do
        [ -e "$f" ] || continue
        if grep -qP '^(\s+|[^#\[=][^=]*)$' "$f" 2>/dev/null; then
            nm=$((nm + 1))
            [ "$nm" -le 4 ] && ml="$ml$(basename "$f") "
            case "$f" in "$HOME"/*) mu="$mu$f ";; esac
        fi
        grep -q '^NoDisplay=true' "$f" 2>/dev/null && continue
        grep -q '^OnlyShowIn=' "$f" 2>/dev/null && continue
        exe=$(awk -F= '/^TryExec=/{print $2; exit}' "$f")
        [ -n "$exe" ] || exe=$(awk -F= '/^Exec=/{print $2; exit}' "$f" | awk '{for (i = 1; i <= NF; i++) {if ($i == "env" || index($i, "=")) continue; print $i; exit}}')
        exe=${exe%\"}; exe=${exe#\"}
        [ -n "$exe" ] || continue
        if ! command -v "$exe" >/dev/null 2>&1 && [ ! -x "$exe" ]; then
            nb=$((nb + 1))
            [ "$nb" -le 6 ] && bl="$bl$(basename "$f") "
            case "$f" in "$HOME"/*) nbu=$((nbu + 1)); bu="$bu$f ";; esac
        fi
    done
done
echo "desktopbroken|$nb|$nbu|$(echo "$bl" | tr '|' '/')"
echo "desktopmalformed|$nm|$(echo "$ml" | tr '|' '/')"
echo "desktopmalformeduser|$(echo "$mu" | tr '|' '/')"
echo "desktopbrokenuser|$(echo "$bu" | tr '|' '/')"

# --- Broken paths ------------------------------------------------------------
missing_paths=""
IFS=: read -ra pdirs <<< "${PATH:-}"
for pd in "${pdirs[@]}"; do
    [ -n "$pd" ] && [ ! -d "$pd" ] && missing_paths="$missing_paths$pd "
done
echo "pathdirs|$(echo "$missing_paths" | wc -w)|$(echo "$missing_paths" | tr '|' '/')"

dangling_user=""; dangling_root=""
for d in "$HOME/.local/bin" "$HOME/bin"; do
    [ -d "$d" ] || continue
    while IFS= read -r l; do dangling_user="$dangling_user$l "; done < <(find "$d" -maxdepth 1 -xtype l 2>/dev/null | head -15)
done
while IFS= read -r l; do dangling_root="$dangling_root$l "; done < <(find /usr/local/bin -maxdepth 1 -xtype l 2>/dev/null | head -15)
echo "danglinguser|$(echo "$dangling_user" | wc -w)|$(echo "$dangling_user" | tr '|' '/')"
echo "danglingroot|$(echo "$dangling_root" | wc -w)|$(echo "$dangling_root" | tr '|' '/')"

# --- Packages / distro -------------------------------------------------------
osid=$(awk -F= '$1 == "ID" {gsub("\"", "", $2); print $2}' /etc/os-release 2>/dev/null)
echo "osid|$osid"
echo "kernel|$(uname -r)"
echo "cachyrepos|$(grep -c '^\[cachyos' /etc/pacman.conf 2>/dev/null)"

# Foreign (AUR/local) packages that a configured repo now provides — on
# CachyOS that repo build is also the optimized (v3/znver) one
nf=0; frl=""
for p in $(pacman -Qmq 2>/dev/null); do
    case "$p" in caelestia++*|*-debug) continue ;; esac
    r=$(pacman -Si "$p" 2>/dev/null | awk -F': *' '/^Repository/{print $2; exit}')
    if [ -n "$r" ]; then
        nf=$((nf + 1))
        [ "$nf" -le 12 ] && frl="$frl$p($r) "
    fi
done
echo "foreignrepo|$nf|$frl"

if [ -n "$(find /var/lib/pacman/sync -name '*.db' -mtime -14 2>/dev/null | head -1)" ]; then
    echo "dbage|fresh"
else
    echo "dbage|stale"
fi
pn=$(find /etc -name '*.pacnew' -o -name '*.pacsave' 2>/dev/null | head -20)
echo "pacnew|$(printf '%s' "$pn" | grep -c .)|$(printf '%s' "$pn" | head -6 | tr '\n' ' ' | tr '|' '/')"
if [ -e /var/lib/pacman/db.lck ] && ! pgrep -x pacman >/dev/null; then echo "paclock|stale"; else echo "paclock|ok"; fi
echo "orphans|$(pacman -Qtdq 2>/dev/null | grep -c .)|$(pacman -Qtdq 2>/dev/null | head -10 | tr '\n' ' ')"
echo "paccache|$(du -sBG /var/cache/pacman/pkg 2>/dev/null | cut -f1 | tr -d G)|$(command -v paccache >/dev/null && echo 1 || echo 0)"
# Per-file warnings on stderr carry the reason; count only files that are
# actually gone. Running unprivileged, root-only paths (/boot, /var/named)
# read as "Permission denied" — those are fine, root sees them.
cor=$(LC_ALL=C pacman -Qk 2>&1 >/dev/null | awk -F': ' '/No such file or directory/ {print $2}' | sort -u) || true
echo "corrupt|$(printf '%s' "$cor" | grep -c .)|$(printf '%s' "$cor" | head -8 | tr '\n' ' ')"

# --- System health -----------------------------------------------------------
echo "failed|$(systemctl --failed --no-legend --plain 2>/dev/null | grep -c .)|$(systemctl --failed --no-legend --plain 2>/dev/null | awk '{print $1}' | tr '\n' ' ')"
echo "userfailed|$(systemctl --user --failed --no-legend --plain 2>/dev/null | grep -c .)|$(systemctl --user --failed --no-legend --plain 2>/dev/null | awk '{print $1}' | tr '\n' ' ')"
echo "journal|$(journalctl -b -p err -q --no-pager 2>/dev/null | grep -c .)"
echo "coredumps|$(timeout 5 coredumpctl list --since -24h --no-legend -q 2>/dev/null | grep -c .)"
echo "disk|$(df --output=pcent / 2>/dev/null | tail -1 | tr -d ' %')"
echo "swap|$(awk '/SwapTotal/{print $2}' /proc/meminfo)"

rootdev=$(findmnt -no SOURCE / 2>/dev/null | sed 's/\[.*//')
ssd=0
[ -n "$rootdev" ] && [ "$(lsblk -dno rota "$rootdev" 2>/dev/null | tr -d ' ')" = "0" ] && ssd=1
systemctl is-enabled -q fstrim.timer 2>/dev/null && trim=1 || trim=0
echo "fstrim|$trim|$ssd"

systemctl is-active -q rtkit-daemon 2>/dev/null && echo "rtkit|active" || echo "rtkit|inactive"
echo "governor|$(cat /sys/devices/system/cpu/cpufreq/policy0/scaling_governor 2>/dev/null)"
if locale 2>&1 >/dev/null | grep -q .; then echo "locale|bad"; else echo "locale|ok"; fi
echo "ntp|$(timedatectl show -p NTP --value 2>/dev/null)"

# --- Audio -------------------------------------------------------------------
systemctl --user is-active -q pipewire 2>/dev/null && echo "pipewire|active" || echo "pipewire|inactive"
if command -v pactl >/dev/null; then
    echo "dupsinks|$(timeout 5 pactl list short sinks 2>/dev/null | awk '{print $2}' | sort | uniq -d | tr '\n' ' ')"
else
    echo "dupsinks|"
fi
