pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Caelestia
import qs.utils

// Toggle for using the laptop somewhere airflow is restricted (e.g. in bed),
// where the fans can't dispose of heat as fast as usual. It swaps in a far
// more sensitive fan curve and changes nothing else: power profile, CPU
// boost and clocks are left alone, so the machine still performs normally —
// pick whatever profile you like alongside it, Performance included.
//
// The curve itself is applied outside the shell's privilege boundary: this
// singleton only flips a plain state file that a root-owned systemd path
// unit watches. See system/bed-mode/ for the fan-curve half of this feature
// and the one-time root setup it needs.
Singleton {
    id: root

    readonly property string statePath: `${Paths.state}/bed-mode`
    readonly property bool enabled: stateFile.checked

    function setEnabled(value: bool): void {
        if (value === root.enabled)
            return;

        stateFile.checked = value;
        stateFile.setText(value ? "1\n" : "0\n");

        Toaster.toast(value ? qsTr("Bed mode enabled") : qsTr("Bed mode disabled"), value ? qsTr("Aggressive fan curve on — performance untouched") : qsTr("Firmware fan curve restored"), "bed");
    }

    // Exposed so a front end outside the shell can flip it — the Tauri bar's
    // battery popout carries this switch, the same as the shell's own does.
    // Going through here rather than writing the state file directly is what
    // keeps the toast and the in-shell UI in step with it.
    IpcHandler {
        target: "bedMode"

        function toggle(): void { root.setEnabled(!root.enabled); }
        function status(): string { return root.enabled ? "on" : "off"; }
    }

    Process {
        id: ensureStateDir

        command: ["mkdir", "-p", Paths.state]
    }

    FileView {
        id: stateFile

        property bool checked: false

        path: root.statePath
        watchChanges: true
        printErrors: false

        onLoaded: checked = text().trim() === "1"
        onFileChanged: reload()
    }

    Component.onCompleted: ensureStateDir.running = true
}
