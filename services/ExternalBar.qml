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

    // How many times in a row it may die before the shell gives up. A bar
    // that cannot start is a bug to fix, not a thing to respawn forever.
    readonly property int maxRetries: 6
    // How many before trying it the other way first.
    readonly property int softRetries: 2
    property int failures
    // WebKit renders through DMA-BUF by default, and on some drivers that
    // trips the compositor's explicit sync: a machine with two GPUs died here
    // every time with "Missing acquire timeline", so the bar never came up
    // and the QML one had already stood down behind it. The fallback costs a
    // little performance and is the difference between a bar and no bar, so
    // it is worth trying before giving up.
    property bool degraded

    Process {
        id: bar

        running: root.installed
        command: [root.binary]

        // CAELESTIA_SHELL_MANAGED tells the bar the shell started it, which
        // is what makes it die with the shell. Run by hand it behaves as
        // it always did.
        environment: root.degraded ? ({
            CAELESTIA_SHELL_MANAGED: "1",
            WEBKIT_DISABLE_DMABUF_RENDERER: "1"
        }) : ({
            CAELESTIA_SHELL_MANAGED: "1"
        })

        // Whatever the bar says goes in the shell's log. It is a child of this
        // process and nobody is watching its console, so without this a bar
        // that starts but cannot reach something has no way to say so.
        stderr: SplitParser {
            onRead: line => console.warn(`bar: ${line}`)
        }

        onExited: code => {
            if (!root.installed)
                return;

            // An install replaces the binary and stops the old process, so
            // being asked to stop is a reinstall wanting to be picked up
            // rather than anything going wrong. Counting SIGTERM as a failure
            // meant a few installs in a row used up the retries and the shell
            // gave up on a bar that was working perfectly well.
            if (code === 0 || code === 15) {
                root.failures = 0;
                respawn.restart();
                return;
            }

            root.failures++;
            if (root.failures === root.softRetries && !root.degraded) {
                root.degraded = true;
                console.warn(`ExternalBar: ${root.binary} keeps exiting ${code}; retrying without WebKit's DMA-BUF renderer`);
            } else if (root.failures >= root.maxRetries) {
                console.warn(`ExternalBar: ${root.binary} exited ${code} ${root.failures} times running; leaving it down`);
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
