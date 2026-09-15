pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Which bar draws the strip along the top, and the shell's ownership of it.
//
// The bar is its own process — a Tauri app on a layer surface, in bar/ — for
// the same reason the launcher is: a webview cannot live inside Quickshell.
// The QML bar is still in the tree, still complete and still works; it simply
// stands down while the external one is installed, the way `Launcher` does.
//
// It is still the shell's bar, though, so the shell runs it: started when the
// shell starts and stopped when it stops. It used to be started once by the
// installer and then outlive every restart, which went wrong in both
// directions — a killed shell left a bar on screen with nothing behind it,
// and after a reboot nothing started one at all, so the QML bar stood down
// behind a bar that was not running and the screen had no bar on it.
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

    // How many times in a row it may die immediately before the shell stops
    // trying. A bar that cannot start is a bug to fix, not a thing to respawn
    // a thousand times a minute.
    readonly property int maxRetries: 5
    property int _failures

    Process {
        id: bar

        running: root.installed
        command: [root.binary]

        // Tells the bar it is being run by the shell, which is what makes it
        // set PR_SET_PDEATHSIG and die with it. Run by hand from a terminal
        // it behaves as it always did.
        environment: ({
            CAELESTIA_SHELL_MANAGED: "1"
        })

        onExited: (code, status) => {
            if (!root.installed)
                return;

            // An install replaces the binary and kills the old process, so an
            // exit is usually a reinstall asking to be picked up rather than
            // anything going wrong.
            if (code === 0) {
                root._failures = 0;
            } else if (++root._failures >= root.maxRetries) {
                console.warn(`ExternalBar: ${root.binary} exited ${code} ${root.maxRetries} times running; leaving it down`);
                return;
            }
            respawn.restart();
        }
    }

    Timer {
        id: respawn

        // Long enough that a binary being swapped underneath is finished
        // being written, short enough that a restart is not noticed.
        interval: 600
        onTriggered: bar.running = true
    }
}
