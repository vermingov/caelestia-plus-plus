# max-perf — "Maximum performance" toggle, root half

The features-menu toggle (bar wrench → "Maximum performance") writes `0`/`1`
to `~/.local/state/caelestia/max-perf`. Everything privileged happens here,
mirroring `system/bed-mode/`:

```
state file → max-perf-sync.path (inotify) → max-perf-sync
                ├─ ON:  max-perf.service      (applier loop, every 5s)
                │       thinkfan-max.service  (aggressive fan curve)
                └─ OFF: stop both; ExecStopPost restores every knob
```

## What the applier does (every 5s — the EC claws limits back otherwise)

| Knob | Max-perf value | Restored to |
|---|---|---|
| ryzenadj STAPM / fast / slow | AC: 42 / 50 / 45 W, Tctl 95 · battery: 30 / 35 / 32 W, Tctl 90 · SMU max-performance | EC reclaims (~seconds) |
| CPU governor (amd_pstate) | `performance` (pinned) | `powersave` |
| CPU boost | on | on |
| ACPI platform profile | `performance` | via ppd `balanced` |
| power-profiles-daemon | `performance` | `balanced` |
| amdgpu dpm force level | `high` (680M clocks pinned) | `auto` |
| Fan | thinkfan-max: level 5 floor, 7 @42C, disengaged @48C | EC auto curve |

Bed-mode's fan service is stopped when this engages (`Conflicts=` +
sync-script stop): two thinkfan instances would fight over
`/proc/acpi/ibm/fan`. Bed mode may stay on underneath: this curve is louder
at every temperature, and `max-perf-sync` restarts bed mode's curve when this
mode turns off.

## Install (one-time, root)

```sh
sudo ./install.sh
```

## Safety posture

- The SMU still throttles at Tctl 95 (90 on battery) — below the silicon's
  100C hard limit; firmware thermal shutdown protections are untouched.
- On battery the limits drop to a battery-safe tier automatically: 50W
  sustained would hammer discharge current and battery wear.
- VRM/current limits are never touched, and if fan control ever fails the
  EC's automatic curve is still running underneath.
- Fan noise and battery drain are the point. Don't leave it on unplugged.
