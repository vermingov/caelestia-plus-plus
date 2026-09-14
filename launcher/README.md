# caelestia-launcher

The application launcher, as a Tauri app: a Rust backend and a Vue frontend in
a WebKitGTK webview.

## Why it is a separate process

It is resident. The window is built once at startup and then shown and hidden,
so opening it is a compositor commit rather than a program start — the same
reason the shell preloads its own launcher rather than instantiating it on
open. `--toggle` does not start anything: it connects to the running one over
a socket in `$XDG_RUNTIME_DIR` and asks it to show itself.

## The glass

The frost behind the window is the compositor's. Hyprland blurs whatever is
under a layer surface, and nothing inside a webview can see the desktop to
blur it — `backdrop-filter` only reaches page content. So the CSS draws the
*pane*: a smoky body, light gathering along the lit lip, a rim that is
brightest where it faces the light and almost gone where it does not, and one
specular streak. The selection row is the one place `backdrop-filter` earns
its cost, lensing the panel behind it the way glass on glass actually does.

Dark glass is mostly dark. The failure mode is edges bright enough to read as
a white outline.

## Layout

    src/            the Vue frontend
    src-tauri/      the Rust backend
      apps.rs       desktop entries, read off disk
      search.rs     ranking: prefix beats word start beats subsequence
      usage.rs      how often each app is launched, decaying over a fortnight
      icons.rs      icon name to file, cached per session
      lib.rs        the Tauri app, the layer surface, the control socket

## Building

    ./install.sh

Needs `cargo`, `npm`, and WebKitGTK 4.1. `gtk-layer-shell` is optional but
wanted: with it the window is an overlay layer surface with the keyboard to
itself; without it, an always-on-top window.
