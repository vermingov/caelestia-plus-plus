# cae

The Caelestia++ shell, rebuilt in Rust on [GPUI](https://www.gpui.rs/) with
[gpui-rsx](https://github.com/wsafight/gpui-rsx) markup: one process, no web
engine, nothing kept warm that is not on screen.

It is taking Quickshell's place a piece at a time. What it draws today:

| | |
|---|---|
| the bar | workspaces, media, tray, dials, clock, status icons, per output |
| its popouts | thirteen panels, the tray's own menus, the Wi-Fi password prompt |
| the launcher | apps, `>` commands, calculator, schemes, variants, wallpapers |
| notifications | toasts, the centre, do-not-disturb, the hot corner |
| the settings | an ordinary window: wallpaper and colours, network, Bluetooth, audio, the compositor, updates, panels, apps, services, units |
| the dashboard | rises from the foot of the screen at the left: the day, media with its words, performance, weather |
| the session menu | logging out and the ways of putting the machine down, with the screen and the keyboard to itself while it is up |
| the on-screen display | how loud and how bright, at the right-hand edge: up when either changes or the edge is reached for, and its bars can be dragged and scrolled |
| the background | the wallpaper, one fading into the next, or the helix that stands in where there is none — and the desktop clock over it, where the settings ask for one |
| the utilities | keeping the machine awake, the screen recorder and its recordings, and the switches: the radios, the microphone, game mode, quiet, the settings |
| the security centre | what the two guards are doing, what they remember, and what starts itself — and the prompt that answers a frozen program |
| the features menu | the machine's modes, behind the wrench |
| the picker | a piece of the screen, for a shot of less than all of it |
| the lock screen | a real session lock, with the password asked through PAM |
| idleness | what happens after a while: the lock, the screens off, the machine down |
| the system scan | what the shell needs from the machine and whether it is there — with a fix per finding that is read before it is run, and one word about it a minute after startup when something is actually missing |
| the battery | the warnings on the way down, and the hibernate before the charge runs out |
| the eggs | the desktop one, drawn in GPUI from the shapes its files were; and the five-act cinema, drawn on the card from a thread of its own like the background (see below) |

What is still Quickshell's: nothing that draws. What is left in `modules/`
is the debug console, which is Quickshell's own log and goes with it, and the
plumbing that makes QML singletons exist. The global shortcuts move to the
compositor's own binds (`STANDALONE.md`). The notification sidebar is gone:
cae's centre is the same list, out of the same edge.

The desktop clock keeps its settings — on, scale, position, the plate and its
opacity — but not `invertColors` or `background.blur`: cae has one palette
rather than Material You's two, and the blur behind the plate is the
compositor's, from the rule that already blurs every other panel.

The plate is on unless it is switched off, where upstream had it off unless
switched on. Upstream's clock shadowed the glyphs themselves, which kept it
readable over anything; GPUI has no shadow for glyphs, only for boxes, so
without a plate a bright wallpaper eats the type. `shadow.enabled` is the
depth under that plate, and draws nothing without one.

## Handing a piece over

For each piece there is a moment when both shells could draw it: the new cae
is installed, and the Quickshell that is running was started before it knew
to stand down. So cae asks first (`core/src/handover.rs`): `qs ipc call cae
stoodDown dashboard`. A Quickshell from before the question existed answers
"Target not found", which is a no, and cae leaves the piece alone and passes
its keys along. After `cae restart` the answer is yes. The installer's marker
files under `$XDG_STATE_HOME/caelestia/` (`bar-serves-notifs`, and
`bar-serves-dashboard`, `-session`, `-osd`, `-background`, `-utilities`) are
what Quickshell reads to know what to stand down. `ExternalBar.asked(piece,
"show"|"hide"|"toggle")` is how its keys and its IPC hand one over.

## The lock screen

A session lock, not a panel over the desktop: the compositor is asked to stop
showing everything else, and it goes on hiding it even if this process dies.
GPUI has no window kind for that protocol, so the shell builds against a copy
of GPUI with one added (`vendor.sh`, `patches/gpui-session-lock.patch`) —
about three hundred lines, mirroring the layer-shell branch it sits beside.
The one thing that is not a mirror: a session lock surface may not be
committed before it has a buffer, so the commit that kicks every other
window off is skipped for this one.

The password goes to PAM, which is how everything else on the machine asks:
the same rules, the same lockout, and no need for the shell to be
privileged. `loginctl lock-session` and `loginctl unlock-session` both reach
it, which is also the way back in if anything here ever goes wrong: from a
terminal or over ssh, `loginctl unlock-session`.

## The background

Not GPUI's. The helix is a lit, focused scene of a few hundred shapes sixty
times a second, and a picture the size of a screen is one GPUI would keep a
second copy of in memory. So the background has a thread, a Wayland
connection and a layer-shell surface of its own under everything
(`app/src/background/easel`), and draws there on Vulkan directly — through
wgpu, the per-frame bookkeeping cost more than the drawing. It shares nothing
with the interface but what it is told to show: the wallpaper `caelestia
wallpaper` last set, where `background.wallpaperEnabled` says so and the
file is there, and otherwise the helix (`dnaEnabled`, in the scheme's primary
colour or `dnaCustomColor`).

The helix is a model of the molecule rather than a picture of one: B-DNA's
proportions, ten and a half base pairs a turn and a wide and a narrow groove,
seen through a lens that keeps its middle sharp and lets its ends go soft,
the far one into the dark. Its shapes are worked out on the processor and
sorted from far to near; the card shades each where it lies, and nothing is
worked out for a pixel no shape covers.

It turns only on a screen whose desktop can be seen, which is one with
nothing tiled or fullscreen on the workspace it shows, and only when the
compositor asks for a frame: at the screen's rate up to sixty a second, and
not at all while the screen is off or the session is locked. Its commands
are recorded once per image; a frame writes the shapes and hands the image
to the compositor as a dma-buf, or through a swapchain where the compositor
or the card cannot take that (`CAE_BACKGROUND_SWAPCHAIN=1` asks for the
swapchain regardless). About one percent of a core while it turns, nothing
while it does not. A picture is cut to each screen's own pixels as it is
hung, drawn while it takes over from the last one, and not again.

What any of this needs is shared: the card, its frames, the swapchain and
the hand-over in `app/src/card/`, the Wayland connection and its layer
surfaces in `app/src/desk.rs`. The cinema is the other thing drawn that way
(`app/src/ui/eggs/cinema/`): one surface over everything on the screen
somebody is looking at, seen through and pressed through, for as long as its
track plays. Every frame is a buffer of marks — strokes of light, motes out
of focus, a flag of real cloth, eyes, a night sky with fireworks over a city
— that one shader works out a pixel at a time, and the portrait
(`portrait.svg`) is painted once with resvg as it starts. About four percent of a core at sixty
frames a second where the GPUI one took all of one, and nothing at all once
it is over.

## The utilities

`cae-shell utilities` opens it at the foot of the screen on the right: three
cards, and no edge to reach for — it is a key's panel, and it goes when the
pointer leaves it or never comes.

Keeping the machine awake is `zwp_idle_inhibit`, which GPUI does not speak
either: one transparent pixel of layer surface in the overlay with an
inhibitor on it (`app/src/awake.rs`), held for as long as the switch is on
and dropped with the connection when it is off. The recorder is
`caelestia record` — region and sound are two chips rather than a menu of
four — and how long it has been going is read from the file it is writing,
so it is right after a restart of the shell. The recordings are the `.mp4`s
in the recordings folder, newest first; the bin asks a second time before it
throws one away.

## The dashboard

Reaching for the foot of the screen at the left opens it (a strip two pixels
tall is all that is there otherwise), and it goes when the pointer does.
`cae-shell dashboard [show|hide|toggle]` is the key's way in; opened that way
it stays until it has been touched. The forecast is kept between openings and
fetched again when it is half an hour old; temperatures, disks, traffic, the
player's position and the words to the song are read only while their page
is up.

## The settings

`cae settings`, or `cae settings audio` for a page by name; the launcher's
`>settings`; "Open settings" on a panel. The QML shell's own ways in
(`WindowFactory.create()`, which the `nexus` IPC target and the utilities
card call) hand over to it while cae is the shell that is running.

Nearly every setting is a key in one of two files, `shell.json` and
`prefs.json`, and the QML shell watches both: a switch here is written to the
file and both shells follow it. The compositor's are the exception, and go
through `caelestia-tools hyprmod`, which owns the Hyprland config. A page
that is only a list of such keys is data (`ui/settings/schema.rs`); a page
that does more is a view of its own (`ui/settings/pages/`).

What the old settings had and these do not, because nothing in cae honours
it yet: the bar's auto-hide and drag threshold, scroll actions, the tray's
and the clock's options, and the logo's size and offsets. They come back
with the thing they configure.

On Hyprland the window wants one rule, or it tiles:

```lua
hl.window_rule({ match = { class = "caelestia-settings" }, float = true })
```

## Standing on its own

`STANDALONE.md` is what is left between this and a desktop with no
Quickshell on it: a service unit (`./install.sh --standalone`) and the keys
repointed at the door. Everything the QML shell drew, cae draws — the lock
screen included, through a GPUI fork that speaks `ext-session-lock-v1`.

## How it runs

Quickshell starts it. `services/ExternalBar.qml` runs the first of `cae-shell`
and `caelestia-bar` it finds installed, as its child, restarts it when it
exits, and stands its own bar, launcher and notification server down while
one is there. The lock screen's notification dock and the keybinds talk to it
over the same two sockets the Tauri bar answered on:

- `$XDG_RUNTIME_DIR/caelestia-launcher.sock`: `show [query]`, `hide`, `toggle`
- `$XDG_RUNTIME_DIR/caelestia-notifs.sock`: the feed out, `centre`, `dnd`,
  `clear`, `close <id>` in
- `$XDG_RUNTIME_DIR/caelestia-shell.sock`: everything else it can be asked
  for: `settings [page]`, and `show`, `hide` or `toggle` for each of
  `dashboard`, `session`, `osd` and `utilities`. `cae-shell settings` is the
  same words from a command line

```
./install.sh              build (release) and install ~/.local/bin/cae-shell
cae restart               hand over, the first time: which bar runs is decided
                          when Quickshell starts
./install.sh --uninstall  back to the Tauri bar, or to the QML one without it
cae preview               look at a build under the bar in use, touching nothing
```

`cae` (the updater) rebuilds it with the CLI and the tools. An install that
changes nothing restarts nothing.

## Layout

- `core/`: what the shell knows about the desktop, with nothing in it about
  how that is drawn. Most of it is the Tauri bar's sources by `#[path]`
  (`../bar/src-tauri/src`), with that bar's glue behind its `tauri-ui`
  feature, so that a fix to one is a fix to both. What only cae needs is in
  `core/src/`: the settings files, the compositor's helper, monitors,
  per-app volumes, thumbnails. **A change there
  is a change to both shells**: check the other with
  `cargo check --features layer-shell` in `bar/src-tauri`.
- `app/`: the binary. `ui/bar`, `ui/popout`, `ui/launcher`, `ui/notifs`,
  `ui/settings`, `ui/dashboard`, `ui/utilities`, `ui/session.rs`, `ui/osd.rs`,
  `background/`, `ui/eggs/cinema` and `awake.rs` (which are not GPUI's, see
  above; the first two draw with `card/` and `desk.rs`), and what they share:
  the text field
  (`ui/field.rs`), the controls (`ui/controls.rs`), the slider, the dial.
- `rig/`: a desktop nobody is looking at, for trying the shell on. See below.

`cargo build --profile fast` is the working build: dependencies optimised as
they ship, this workspace's crates compiled quickly. One to two seconds.

## Trying it without touching the desktop

`rig/rig` runs the shell under a headless sway, fetched from the
distribution's mirror into `~/.cache/cae-rig`, with a virtual pointer and
keyboard, and with `wpctl`, `nmcli`, `bluetoothctl` and the rest replaced by
shims that pass reads through and write down what they were asked to change.

```
rig/rig cae                       the shell, as a preview
rig/rig serve                     the shell for real, on a session bus of its
                                  own with a history that is not yours
rig/rig move 1675 31              hover the clock
rig/rig notify -a Mail "Hello"    a notification, to the shell `serve` started
rig/rig settings audio            the settings, on a page
rig/rig dashboard show            the dashboard
rig/rig session                   the session menu: its commands are shims too
rig/rig level speakers 42         a pretend volume, moved, which is what brings
                                  the on-screen display up; yours is not touched
rig/player                        a player that plays nothing, on the bus
                                  `rig serve` made, for the media page
rig/hyprland [window|bare]        a Hyprland with a fullscreen window in front
                                  (or a tiled one, or none), for a shell
                                  started with RIG_HYPRLAND=rig
rig/rig kept                      the shell.json the shell inside has written
rig/rig shot out.png "1440,0 480x300"
rig/rig asked                     what it tried to change, and did not
rig/rig down
```

The shell inside reads and writes a copy of the settings, made as it starts,
so a switch pressed in there changes nothing of yours. `hyprctl` is a shim
too, and says there are two monitors, which is what the page that arranges
them needs to be tried on.

Not shimmed, because they are D-Bus calls and not programs: the power-profile
dials, the media transport and a tray menu's entries. Those act on the real
machine from inside the rig.

## The shell it replaced

Still in the tree and still working: the Tauri bar in `bar/`, and the QML bar,
launcher and notification server under it in `modules/` and `services/`. The
branch `before-cae` is the checkout as it was before any of this. A branch and
not a tag: the shell's update check fetches with `--prune-tags`, which deletes
every local tag the remote does not have.
