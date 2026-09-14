# The fast half of the Caelestia CLI

`caelestia` is on the other end of thirteen keybinds — every special-workspace
toggle, Print, the clipboard and emoji pickers, the recorder — and the Python
implementation spends about **110 ms importing itself** before it can look at
its own arguments. That is the whole of the delay between pressing a key and
anything happening.

This is the same commands without the interpreter:

| command | Python | Rust |
| --- | --- | --- |
| `caelestia emoji` | 112 ms | 1.6 ms |
| `caelestia shell -s` | 141 ms | 30 ms (all of it `qs`) |
| anything not implemented here | 110 ms | 110 ms + ~1 ms |

## It is a front-end, not a fork

It answers `shell`, `toggle`, `screenshot`, `record`, `clipboard` and
`emoji`. Everything else — `scheme`, `wallpaper`, `install`, `update`,
`resizer` — is handed to the Python CLI with its arguments untouched, by
`exec`, so there is one definition of what those commands do and no second
process left in the tree.

The same applies to anything it cannot parse. An unknown subcommand, an
unknown flag, a missing argument: all of them fall through rather than being
guessed at, so a new option added on the Python side works the day it lands.
The worst this binary can do is not be used.

## Install

```sh
./install.sh              # builds, then installs to ~/.local/bin/caelestia
./install.sh --uninstall  # removes it; the Python CLI is in charge again
```

No root. `~/.local/bin` precedes `/usr/bin` on PATH, so the keybinds find this
one; the packaged CLI stays where pacman put it, which is also what this
binary falls back to.

## Working on it

```sh
cargo test
cargo clippy --all-targets
```

The tests cover the parts where being wrong is silent: which arguments belong
to this binary and which fall through, the window-matching rules the toggles
use (a class match is a *substring* — "discord" is meant to match
"discord-canary"), and the exact dispatch strings Hyprland is sent. That last
one matters because a Lua-configured Hyprland takes a line of Lua rather than
a dispatcher name, and it accepts a wrong one in silence.

Where a test needs a live system it uses one: the compositor is read for real
and the clock is checked against `date`. Nothing here dispatches, spawns or
records during a test run.

## What is not here yet

`scheme` and `wallpaper`, which are the other measurable cost: generating a
palette from a wallpaper is ~170 ms of pure-Python colour maths on top of the
110 ms start. Porting those means porting the Material colour pipeline, which
is a bigger piece of work than wrapping a subprocess.
