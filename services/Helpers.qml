pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// The small programs the shell shells out to.
//
// They used to be Python scripts under assets/ — each one paying 25 to 50 ms
// of interpreter start to read a few files, and one of them holding an
// interpreter resident for the whole session. They are a Rust binary now.
//
// The scripts are still there, and are still used when the binary is not
// installed: a checkout without a Rust toolchain behaves exactly as it did.
// Everything that runs one of these waits for `ready` first, so nothing is
// launched with a command that is about to change under it.
Singleton {
    id: root

    readonly property string binary: "caelestia-tools"

    // True once the check below has answered, whichever way
    property bool ready: false
    property bool hasBinary: false

    // tool -> the script that does the same job
    readonly property var scripts: ({
            gpus: "list-gpus.py",
            startup: "startup-ctl.py",
            "egg-watch": "penis-egg-watch.py",
            hyprmod: "hyprmod-ctl.py",
            "config-doctor": "config-doctor.py"
        })

    // The command to run `tool`, with `args` appended.
    function command(tool: string, args: var): var {
        const extra = args ?? [];
        if (root.hasBinary)
            return [root.binary, tool, ...extra];
        return ["python3", Quickshell.shellPath(`assets/${root.scripts[tool]}`), ...extra];
    }

    Process {
        running: true
        command: ["sh", "-c", `command -v ${root.binary} >/dev/null`]

        onExited: code => {
            root.hasBinary = code === 0;
            root.ready = true;
        }
    }
}
