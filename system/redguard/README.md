# Redguard — behavioral process protection

A lightweight HIPS for this Caelestia setup, companion to redwall. Redwall
governs the network (who may connect out); redguard governs behavior (is a
process acting like an exploit payload). It touches no networking, so it is
completely VPN-agnostic.

## What it catches

Two deliberately narrow, high-confidence detections — chosen so false
positives are near zero, because every hit freezes a real process:

- **reverse-shell** — an interpreter/shell (bash, python, perl, nc, socat, …)
  whose stdin/stdout/stderr is wired to a **network** socket. The textbook
  reverse shell. Normal interactive shells (pty stdio) and pipelines (pipe
  stdio) never match.
- **foreign-exec** — a process whose executable file lives in a world-writable
  scratch dir (`/tmp`, `/dev/shm`, `/var/tmp`, `/run/user`) or has been
  deleted/anonymised (memfd, unlinked) while running. The dropper and
  in-memory-malware pattern. AppImage mounts, Chromium/Electron sandboxes and
  systemd-private paths are excluded.

The spawning parent (e.g. "spawned by firefox") is reported for context but is
never a trigger by itself — browsers legitimately run helper shells, so lineage
alone would be a false-positive machine.

## How it works

```
exec()  ->  kernel netlink proc-connector (real time, no polling)
        ->  pre-filter: interpreter, or exe in scratch/deleted?  no -> ignore
        ->  classify from /proc (unfrozen: benign execs are never signalled,
              so foreground shell jobs keep their terminal)
              benign        -> ignore
              detection     -> SIGSTOP (freeze) -> re-classify -> prompt the
                               bar (allow / block / once)
        verdict  ->  allow: SIGCONT   block: SIGKILL (group)   once: SIGCONT
                     allow/block remembered per-executable, persisted
```

## Pieces

```
../rust/redguardd   the daemon: proc connector, detections, freeze, UI socket
../rust/redcommon   shared with redwall: JSON, UI socket, rule store, /proc
redguardd.py        the Python daemon it replaces, kept as a fallback
redguardd.service   systemd unit (root; runs at boot, Restart=on-failure)
install.sh          one-time root setup (builds the Rust daemon, falls back)
uninstall.sh        full removal
```

The daemon is Rust with no dependencies at all: the netlink proc connector,
the `/proc` reads and the signals are written out. It sees every exec on the
machine, so the pre-filter that decides "worth a closer look" runs constantly —
that is not work for an interpreter, and a monitor that costs nothing is a
monitor people leave switched on. Machines with no Rust toolchain still get the
Python implementation; `install.sh` picks whichever it can build.

## Test the UI without root

```sh
../rust/build.sh redguardd    # prints the binary path
../rust/target/release/redguardd --simulate \
    --sock /run/user/$UID/redguard-ui.sock \
    --rules /run/user/$UID/redguard-rules.json --ui-gid $(id -g)
```

Simulate mode never opens the proc connector and never freezes anything. It
seeds two synthetic detections once a UI connects, and takes more on demand:
feed it `{"t":"simdetect","exe":"...","name":"...","kind":"reverse-shell"}`
lines over the socket.

The daemon's own tests cover the detections, the connector's parser and the
freeze/release/kill paths against real processes: `cd ../rust && cargo test`.

Shell side (ships with the config, no install):

```
services/Protection.qml                  socket bridge + models + IPC
modules/protection/ProtectionPrompt.qml  the freeze alert (allow/block/once)
modules/protection/ProtectionTab.qml     rules manager (in the security center)
```

## Honesty / limits

- **Best-effort, not a kernel sandbox.** There is a small window between exec
  and freeze. For the interactive payloads this targets — a reverse shell
  waiting for its operator, a dropper about to act — the freeze lands in time.
  It is not a substitute for not running untrusted code.
- **Fails open.** If the bar UI is not connected there is no one to answer, so
  a frozen process is released and logged rather than stuck forever — on
  detection when no UI is attached, and again the moment the last UI
  disconnects with something still frozen. Enforcement is therefore active only
  while the shell runs (it is the desktop).
- **Loud about being blind.** If the kernel refuses the exec stream (no
  CAP_NET_ADMIN), the daemon exits instead of sitting there with a healthy
  shield in the bar and nothing being watched.
- **Freeze, never silent kill.** Unknown detections always ask. Only an
  explicit remembered "block" kills on sight.

## Safety

Stopping the service stops all protection immediately — it never leaves a
process frozen. Killing the daemon does not affect any running program.
