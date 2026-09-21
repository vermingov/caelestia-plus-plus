# Standing on its own

What is left between cae and a desktop with no Quickshell on it. Nothing
here is done by installing: each step changes something of the user's, and
each is one line.

## 1. Start with the session

```
./install.sh --standalone      # installs and enables cae-shell.service
systemctl --user start cae-shell
```

Then take Quickshell out of the session: in
`~/.config/hypr/hyprland/execs.lua`, remove

```lua
hl.exec_cmd("caelestia shell -d")
```

The unit is `PartOf=graphical-session.target`, so cae starts with the
desktop, comes back if it falls over, and its log is
`journalctl --user -u cae-shell`.

## 2. The keys

Every `caelestia:*` global was a Quickshell shortcut. Each becomes a command
the door answers. In `~/.config/hypr/hyprland/keybinds.lua`:

| was | becomes |
|---|---|
| `global("caelestia:launcher")` | `exec_cmd("cae-shell launcher")` |
| `global("caelestia:session")` | `exec_cmd("cae-shell session")` |
| `global("caelestia:sidebar")` | `exec_cmd("cae-shell centre")` |
| `global("caelestia:clearNotifs")` | `exec_cmd("cae-shell notifs clear")` |
| `global("caelestia:showall")` | `exec_cmd("cae-shell showall")` |
| `global("caelestia:brightnessUp")` | `exec_cmd("cae-shell brightness up")` |
| `global("caelestia:brightnessDown")` | `exec_cmd("cae-shell brightness down")` |
| `global("caelestia:mediaToggle")` | `exec_cmd("cae-shell media play")` |
| `global("caelestia:mediaNext")` | `exec_cmd("cae-shell media next")` |
| `global("caelestia:mediaPrev")` | `exec_cmd("cae-shell media previous")` |
| `global("caelestia:mediaStop")` | `exec_cmd("cae-shell media stop")` |
| `global("caelestia:screenshot")` | `exec_cmd("caelestia screenshot -r")` |
| `global("caelestia:screenshotFreeze")` | `exec_cmd("caelestia screenshot -r -f")` |
| `global("caelestia:lock")` | `exec_cmd("cae-shell lock")` |

The launcher key is `{ release = true }` on SUPER; that stays as it is, only
the command changes.

## 3. The lock screen

Done: cae draws it (`app/src/ui/lock.rs`). It is a real session lock, and
`cae-shell lock` / `caelestia:lock` / `loginctl lock-session` all reach it.
The keybind becomes `exec_cmd("cae-shell lock")`.

The one thing to try before trusting it: the password. A wrong one costs a
`faillock` strike (three and the account waits ten minutes), so try the right
one first, from a session you can get back into. If anything goes wrong,
`loginctl unlock-session` from a terminal or over ssh unlocks it.

## 4. What goes with it

Nothing in `modules/` is reached any more except the easter eggs and the
debug console, and `shell.qml` can go with them.

The System scan has moved: it is a settings page now
(`cae-shell settings scan`, or `cae-shell scan`), with the same probe script,
the same findings, and the same rule that a fix is read before it is run.
`cae-shell scan now` does the startup look by hand. What is left in the debug
window is the shell's own log, which is Quickshell's log, and goes with
Quickshell.

Both eggs came across too (`cae-shell egg`, `cae-shell cinema`), drawn as
paths rather than loaded as SVGs, because GPUI will draw an SVG but will not
turn one and both scenes are mostly turning. The watcher that listens for the
words knocks at cae's door first and falls back to `qs`, so it works either
way round.

`standalone.py` makes the two changes below for you and keeps a copy of each
file beside it; `standalone.py --undo` puts them back. Run it without
arguments first — it says exactly what it would change and changes nothing.
