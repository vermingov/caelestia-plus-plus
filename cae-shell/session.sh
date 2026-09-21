#!/usr/bin/env bash
# Starts cae from the compositor, for a session systemd knows nothing about.
#
# Where the session is run by something that speaks systemd — uwsm, and the
# like — `graphical-session.target` is activated for us, the unit is wanted
# by it, and cae starts without any of this. A Hyprland started straight from
# a display manager activates nothing: the target stays inactive, the unit is
# never pulled in, and the systemd user manager has never heard of the Wayland
# display, so even starting it by hand gives a shell with no compositor to
# draw on.
#
# Both are fixed the same way, and doing it under uwsm too costs nothing: the
# import is what that session already did, and starting a unit that is running
# is not an error.
#
# Hyprland runs this from `execs.lua`, which is how it inherits the
# environment there is to import.
set -uo pipefail

# What a shell needs to find the compositor it draws on. Only the ones that
# are actually set: `import-environment` clears a variable it is asked for and
# cannot find, and clearing WAYLAND_DISPLAY is the failure this exists to
# avoid.
wanted=(WAYLAND_DISPLAY HYPRLAND_INSTANCE_SIGNATURE XDG_CURRENT_DESKTOP XDG_SESSION_TYPE XDG_RUNTIME_DIR)
present=()
for name in "${wanted[@]}"; do
    [[ -n ${!name:-} ]] && present+=("$name")
done
((${#present[@]})) && systemctl --user import-environment "${present[@]}"

# The unit by name, not the target it is wanted by: systemd ships
# graphical-session.target with `RefuseManualStart=yes`, so activating it to
# pull the unit in is refused on every machine there is. Only a session
# manager may start it, which is the whole reason a session without one needs
# this script.
#
# `restart` rather than `start`, because without an active target nothing
# stops the unit when the session ends either: a shell left over from the
# last login is still "running", `start` would call that done, and the
# desktop would be drawn by a process holding a Wayland display that no
# longer exists. Restarting gives every session a shell of its own, and on a
# unit that is stopped it simply starts it.
exec systemctl --user restart cae-shell.service
