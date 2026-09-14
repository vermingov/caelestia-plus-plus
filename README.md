# Caelestia++

A heavily modified, self-owned fork of the [caelestia](https://github.com/caelestia-dots/shell) desktop shell for quickshell. Detached from upstream and from the AUR — updates ship from this repo only, straight into the shell's Settings → Updates tab.

## Install (Arch)

```sh
bash <(curl -fsSL https://raw.githubusercontent.com/vermingov/caelestia-plus-plus/main/install.sh)
```

The installer sets up an AUR helper if needed, installs dependencies, installs the `caelestia++-shell` / `caelestia++-cli` packages from the latest release, and clones this repo to `~/.config/quickshell/caelestia`.

Start with `caelestia shell -d` (Hyprland: `exec-once = caelestia shell -d`).

## Updating

The shell checks this repo periodically; pending commits appear under Settings → Updates with a one-click "Update & restart". Manual equivalent: `git -C ~/.config/quickshell/caelestia pull`.

## If the shell is gone after a system update

Quickshell links against Qt private API, so a Qt patch release can leave the
binary unable to start (`qs --version` fails with `undefined symbol ...
Qt_6_PRIVATE_API`). A running shell keeps working on the old libraries, so the
breakage only shows at the next login. Fix from a terminal:

```sh
sudo bash ~/.config/quickshell/caelestia/system/quickshell/install.sh
```

It builds quickshell against the installed Qt (a few minutes) and installs it
as `caelestia++-quickshell`. The shell's System check offers the same fix, and
the installer runs it automatically when needed.

## Notable differences from upstream

- Floating pill top bar with logo endcap, bar visualiser, workspace numbers
- Resident preloaded panels (no first-open jank)
- Firewall and features modules, DNA shader background, power-aware blur
- Nexus settings: implemented Updates tab wired to this repo

## License

GPL-3.0, same as upstream — see [LICENSE](LICENSE).
