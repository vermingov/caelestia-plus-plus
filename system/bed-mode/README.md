# bed-mode

A fan curve, and nothing else. Bed mode does not touch the power profile,
CPU boost, clocks or voltages — run Performance or Maximum performance
alongside it if you want to; the fans simply work harder to clear a
restricted intake.

Toggle in the battery popout (hover the battery in the topbar) for using the
laptop somewhere airflow is restricted, e.g. in bed. It swaps the firmware's
fan curve for an aggressive one that starts the fans almost at idle, reaches
the EC's top regulated level by ~56C, and drops the rpm cap entirely above
~64C ("disengaged", ~4500-5000rpm). Loud, deliberately: the heat has nowhere
else to go.

Up to v4 it also clamped the power profile to Balanced and held CPU boost
off. Both are gone. Holding the global CPU boost switch off made the kernel
reject every per-policy boost write, which blocked power-profiles-daemon
from switching profiles at all, and clamping the profile made the machine
slow exactly when you were using it. Fans are enough.

## Interaction with the other modes

| | |
|---|---|
| Power profile (Eco/Balanced/Performance) | independent; pick any |
| Dynamic | independent; no ceiling from bed mode |
| Maximum performance | may be on together — its curve is louder at every temperature, so it takes the fans while it runs and `max-perf-sync` hands them back afterwards |
| Anti-Heat | independent (undervolt, gentler curve; yields to this one) |

## How it's wired

The shell (`services/BedMode.qml`) never touches hardware directly — it only
writes `0`/`1` to `~/.local/state/caelestia/bed-mode`, a file it owns. A
root-owned systemd path unit watches that file and drives everything else:

```
Battery popout switch
  -> services/BedMode.qml writes ~/.local/state/caelestia/bed-mode
  -> bed-mode-sync.path (root, inotify) triggers bed-mode-sync.service
  -> bed-mode-sync (root) starts/stops thinkfan-bed.service
  -> thinkfan-bed.service runs thinkfan against thinkfan-bed.yaml,
     driving the EC fan via thinkpad_acpi
```

Stopping `thinkfan-bed.service` hands the fan back to the EC's own automatic
curve, so bed-mode off == stock behaviour.

## One-time setup (requires root)

The shell-side toggle works out of the box; the fan curve behind it needs
root, once:

```sh
paru -S thinkfan   # AUR, not in the official repos
sudo ./install.sh
```

`install.sh` installs the modprobe option, the sensitive curve, and the
systemd units, then prints the last step: `thinkpad_acpi` needs
`fan_control=1` to accept manual fan levels, which only takes effect after a
reboot (or a live `modprobe -r thinkpad_acpi && modprobe thinkpad_acpi`).

## Tuning the curve

`thinkfan-bed.yaml` reads CPU temp (`k10temp`/Tctl) and maps it to fan levels
0-7 via `/proc/acpi/ibm/fan`. Each `[level, lower, upper]` entry drops back a
level below `lower` and steps up past `upper`; edit the numbers and
`systemctl restart thinkfan-bed.service` (only takes effect while bed-mode
is on) to try a different curve. See `man thinkfan.conf`.
