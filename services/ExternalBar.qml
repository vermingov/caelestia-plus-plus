pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Which bar draws the strip along the top.
//
// The bar is its own process now — a Tauri app on a layer surface, in bar/ —
// for the same reason the launcher is: a webview cannot live inside
// Quickshell. The QML bar is still in the tree, still complete and still
// works; it simply stands down while the external one is installed, the way
// `Launcher` does.
//
// The switch is whether the binary is on PATH rather than a config key: the
// config doctor validates shell.json against the schema the plugin compiles
// in, so a key it has never heard of would be offered up for deletion as a
// typo. `bar/install.sh --uninstall` puts this one back in charge.
Singleton {
    id: root

    readonly property string binary: "caelestia-bar"

    // True once the check below has answered, whichever way
    property bool ready: false
    readonly property alias external: root.installed
    property bool installed: false

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
