# Dynamic power mode

Fourth segment in the battery-menu profile selector (next to Eco / Balanced /
Performance). While it is selected, a small daemon continuously picks the best
`power-profiles-daemon` profile for what the machine is actually doing.

## What it is (and isn't)

It only ever sets the ppd profile ({power-saver|balanced|performance}, via
D-Bus: `busctl set-property net.hadess.PowerProfiles ... ActiveProfile`) —
the exact same three profiles the manual buttons set, chosen automatically. It
never touches ryzenadj, the CPU governor, or amdgpu, so it cannot fight
**Anti-Heat** (owns the thermal / PPT caps) or **Max-perf** (owns the 50W pin).
It is "Eco + Balanced + Performance, made automatic".

## How it decides — `dynamic-apply`

Every `TICK` (4s):

- **Load comes from `/proc/stat` deltas, not loadavg.** loadavg on this box is
  I/O-inflated (zram swap, media I/O) and would falsely pin performance while
  idle. `iowait` is excluded from "busy" for the same reason.
- Tracks both **aggregate** busy % and the **hottest single core**, so a
  latency-sensitive single thread escalates instantly despite a small share of
  16 threads.

| Situation | Profile |
|-----------|---------|
| AC + busy (agg ≥30% or a core ≥90%) | performance |
| AC + deep idle (agg <10% and every core <50%) | power-saver |
| AC + everything else | balanced |
| Battery + busy | balanced (ceiling — protects battery/wear) |
| Battery + deep idle | power-saver |
| Battery ≤15% | power-saver (forced) |
| Max-perf on | daemon stands down entirely |

**Hysteresis:** escalates UP immediately (snappy under sudden load); steps DOWN
one tier only after `DOWN_TICKS` (5 × 4s = 20s) of sustained-lower demand, so a
brief lull mid-build does not drop you out of performance.

**Efficiency:** the profile is only set when the pick *changes* (plus a cheap
D-Bus drift check every `DRIFT_TICKS` to heal suspend/ppd-restart clobbers);
idle loop cost is ~0.1% CPU. It deliberately avoids spawning
`powerprofilesctl` — that is a python script costing ~280ms CPU per call.

The daemon publishes the current pick to
`~/.local/state/caelestia/dynamic-tier`, which the battery menu reads to show
"Dynamic → <profile>".

## Tuning

All thresholds are named constants at the top of `dynamic-apply`
(`AGG_HIGH`, `CORE_HIGH`, `AGG_IDLE`, `CORE_IDLE`, `BAT_CRIT`, `DOWN_TICKS`,
`DRIFT_TICKS`, `TICK`). Edit, then `sudo ./install.sh` again (or copy the file to
`/usr/local/bin/` and `systemctl restart dynamic.service`).

## Install

    sudo ./install.sh

Enables `dynamic-sync.path` (watches the toggle file) and `dynamic-sync.service`
(reconciles at boot). The GUI toggle does the rest.
