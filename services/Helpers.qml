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
//
// The check is per tool, not per binary. A checkout moves ahead of its
// installed binary whenever the shell updates without rebuilding, and a tool
// the old binary has never heard of has to fall back to its script rather
// than fail — so the binary is asked what it can do, not merely whether it
// exists. Everything that runs one of these waits for `ready` first.
Singleton {
    id: root

    readonly property string binary: "caelestia-tools"

    // True once the check below has answered, whichever way
    property bool ready: false
    // The tools the installed binary answers to; empty when there is none
    property var available: []

    // tool -> the script that does the same job
    readonly property var scripts: ({
            gpus: "list-gpus.py",
            startup: "startup-ctl.py",
            "egg-watch": "penis-egg-watch.py",
            hyprmod: "hyprmod-ctl.py",
            "config-doctor": "config-doctor.py"
        })

    function has(tool: string): bool {
        return root.available.includes(tool);
    }

    // The command to run `tool`, with `args` appended.
    function command(tool: string, args: var): var {
        const extra = args ?? [];
        if (root.has(tool))
            return [root.binary, tool, ...extra];
        return ["python3", Quickshell.shellPath(`assets/${root.scripts[tool]}`), ...extra];
    }

    Process {
        running: true
        // A binary too old to know `list` prints nothing and exits non-zero,
        // which reads as "no tools" and sends everything to the scripts.
        command: [root.binary, "list"]

        stdout: StdioCollector {
            onStreamFinished: {
                root.available = text.split("\n").map(l => l.trim()).filter(l => l.length > 0);
            }
        }

        onExited: root.ready = true
    }
}
