pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Which launcher the shortcut opens.
//
// The launcher is its own process now — a Tauri app on an overlay layer
// surface, in launcher/ — because a webview cannot live inside Quickshell.
// The QML one is still in the tree and still works.
//
// The switch between them is whether the binary is installed, rather than a
// config key: the shell's own config doctor validates shell.json against the
// schema the plugin compiles in, so a key it has never heard of would be
// offered up for deletion as a typo. `launcher/install.sh --uninstall` puts
// the QML launcher back, and a checkout with no Rust toolchain never leaves
// it.
Singleton {
    id: root

    readonly property string binary: "caelestia-launcher"
    readonly property string sockPath: `${Quickshell.env("XDG_RUNTIME_DIR") || "/tmp"}/caelestia-launcher.sock`

    // True once the check below has answered, whichever way
    property bool ready: false
    readonly property alias external: root.installed
    property bool installed: false

    readonly property bool connected: sockLoader.item?.connected ?? false

    // Commands go down a socket this singleton keeps open.
    //
    // Spawning `caelestia-launcher --toggle` to write eight bytes costs about
    // forty milliseconds — not the launcher's work, but the dynamic linker's,
    // mapping GTK and WebKit into a process that only wants a socket. On a
    // keybind that is the difference between opening and seeming to think
    // about it. The spawn is still there as the fallback for a launcher that
    // is installed but not yet running, which is the one case the socket
    // cannot cover.
    function send(verb: string, query: string): void {
        const line = `${verb} ${query ?? ""}\n`;
        if (root.connected)
            sockLoader.item.write(line);
        else
            Quickshell.execDetached([root.binary, `--${verb}`, query ?? ""]);
    }

    function toggle(): void {
        root.send("toggle", "");
    }

    // Opens straight into a mode, e.g. ">wallpaper ".
    function open(query: string): void {
        root.send("show", query);
    }

    // A Quickshell Socket does not re-attempt after a failed connect, and
    // toggling `connected` on the existing object is a no-op — so the socket
    // lives in a Loader that is rebuilt until a fresh one connects. The
    // launcher is restarted by its own install script, and a connection that
    // never came back would leave the keybind spawning a process for the rest
    // of the session. Same shape as Firewall.qml, for the same reason.
    Loader {
        id: sockLoader

        active: root.installed

        sourceComponent: Component {
            Socket {
                path: root.sockPath
                connected: true

                onConnectionStateChanged: {
                    if (!connected)
                        reconnect.restart();
                }
            }
        }
    }

    Timer {
        id: reconnect

        interval: 2000
        onTriggered: {
            if (root.installed && !root.connected) {
                sockLoader.active = false;
                sockLoader.active = true;
            }
        }
    }

    // The frost behind the pane is the compositor's: a webview can only blur
    // its own page, never what is under the window. Hyprland blurs a layer
    // surface when a rule says to, and that rule used to exist only in the
    // config of whoever wrote the launcher — everyone else got a see-through
    // pane. Set here instead, the way Colours does it for the drawers, so it
    // needs nothing from the user's own files.
    //
    // The surface covers the whole output and is fully transparent outside
    // the pane, so the cutoff keeps the blur on the pane alone. It has to sit
    // under the pane's own alpha (0.72 at its thinnest) and above zero.
    function reloadHyprRules(): void {
        const rule = Hypr.usingLua
            ? `eval hl.layer_rule({ match = { namespace = "${root.binary}" }, %1 = %2 })`
            : `keyword layerrule %1 %2, match:namespace ${root.binary}`;
        Hypr.extras.batchMessage([
            rule.arg("blur").arg(Hypr.usingLua ? "true" : "1"),
            rule.arg("ignore_alpha").arg("0.6")
        ]);
    }

    onInstalledChanged: {
        if (installed)
            reloadHyprRules();
    }

    // A config reload drops every rule that was set at runtime.
    Connections {
        function onConfigReloaded(): void {
            if (root.installed)
                root.reloadHyprRules();
        }

        target: Hypr
    }

    // Asked again after anything installs or removes the binary, because the
    // check below runs once and a shell that learned the answer at startup
    // would keep both this and the other one on screen until it restarted.
    function recheck(): void {
        check.running = true;
    }

    Process {
        id: check

        running: true
        command: ["sh", "-c", `command -v ${root.binary} >/dev/null`]

        onExited: code => {
            root.installed = code === 0;
            root.ready = true;
        }
    }
}
