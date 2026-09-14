pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Caelestia
import qs.services
import qs.utils

// "Maximum performance" mode: pin the machine at its limits. Like BedMode,
// this singleton only flips a plain state file it owns; the privileged work
// (ryzenadj 42/50/45W loop, pinned governor, amdgpu high, thinkfan-max) runs
// root-side via a path unit. See system/max-perf/ for that half; the first
// enable installs it automatically through pkexec (`installed` tracks that),
// because without it this toggle is power-profile-and-eye-candy only and
// the fans never change.
//
// Drags GameMode
// along so the compositor sheds its eye candy too.
Singleton {
    id: root

    readonly property string statePath: `${Paths.state}/max-perf`
    readonly property bool enabled: stateFile.checked
    // True once system/max-perf/install.sh has been run (root path unit exists).
    property bool installed: false
    property bool installing: false

    function setEnabled(value: bool): void {
        if (value === root.enabled)
            return;

        stateFile.checked = value;
        stateFile.setText(value ? "1\n" : "0\n");

        // Bed mode may stay on: its fan curve is a subset of max-perf's, and
        // the root halves hand the fans back and forth
        if (value)
            Dynamic.setEnabled(false); // Dynamic and Max-perf both drive the profile; never both
        PowerDaemon.setProfile(value ? "performance" : "balanced");
        GameMode.enabled = value;

        if (value && !root.installed && !root.installing) {
            root.installing = true;
            Toaster.toast(qsTr("Setting up Maximum performance"), qsTr("Installing the privileged half — enter your password"), "key");
            installer.running = true;
        } else if (!root.installing) {
            Toaster.toast(value ? qsTr("Maximum performance engaged") : qsTr("Maximum performance off"), value ? qsTr("CPU/GPU pinned at their limits, tuned to this machine") : qsTr("Power plan, clocks and fans restored"), "bolt");
        }
    }

    Process {
        id: installer

        // timeout: with no polkit agent the password prompt never appears
        // and pkexec would hang this toggle on "installing" forever
        command: ["timeout", "600", "pkexec", "bash", `${Quickshell.shellDir}/system/max-perf/install.sh`]
        stdout: SplitParser {
            onRead: data => console.info("max-perf install:", data)
        }
        onExited: code => {
            root.installing = false;
            if (code === 0) {
                root.installed = true;
                Toaster.toast(qsTr("Maximum performance ready"), qsTr("Root half installed — the mode is now active"), "bolt");
            } else {
                stateFile.checked = false;
                stateFile.setText("0\n");
                Toaster.toast(qsTr("Setup not completed"), code === 126 || code === 127 ? qsTr("Authentication was dismissed — toggle again to retry") : qsTr("Installer failed (code %1) — see the debug console").arg(code), "warning");
            }
        }
    }

    Process {
        id: ensureStateDir

        command: ["mkdir", "-p", Paths.state]
    }

    Process {
        id: installCheck

        command: ["systemctl", "is-enabled", "--quiet", "max-perf-sync.path"]
        onExited: code => root.installed = code === 0
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

    Component.onCompleted: {
        ensureStateDir.running = true;
        installCheck.running = true;
    }
}
