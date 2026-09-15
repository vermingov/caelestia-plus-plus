pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Caelestia
import qs.services
import qs.utils

// Feature-mode hub behind the bar wrench. Each entry in `features` is one
// toggleable OS mode; the menu (modules/features/FeaturesMenu.qml) renders
// whatever is listed here, so adding a feature = add a descriptor below plus
// its backing implementation (or delegate to an existing service).
//
// Modes owned by this singleton (lidStay, caffeine) persist to
// ~/.local/state/caelestia/features.json and are re-applied when the shell
// starts, so they survive reboots and `caelestia shell -d` restarts alike.
// gameMode / bedMode delegate to their own services, which persist themselves.
Singleton {
    id: root

    property bool menuOpen: false

    readonly property string statePath: `${Paths.state}/features.json`
    readonly property bool lidStay: props.lidStay

    // Drives the menu and the bar badge. Rebuilt whenever any mode flips.
    // Bed mode is in the battery popout (laptops), game mode and caffeine in
    // the utilities cards. Max-perf and anti-heat adapt to the hardware
    // root-side (system/*/…-apply detect chassis, CPU vendor and tools), so
    // both surface on laptops and desktops; enabling either installs its
    // root half through pkexec on first use, no terminal needed.
    readonly property var features: [
        {
            id: "maxPerf",
            name: qsTr("Maximum performance"),
            desc: MaxPerf.installing ? qsTr("Setting up the root half…") : MaxPerf.installed ? qsTr("Pins CPU/GPU at their limits, tuned to this machine") : qsTr("First enable sets up the privileged half (asks for your password)"),
            icon: "bolt",
            enabled: MaxPerf.enabled
        },
        {
            id: "antiHeat",
            name: qsTr("Anti-Heat"),
            desc: AntiHeat.installing ? qsTr("Setting up the root half…") : AntiHeat.installed ? qsTr("Runs cooler at full speed: undervolt and early fans, never power caps") : qsTr("First enable sets up the privileged half (asks for your password)"),
            icon: "ac_unit",
            enabled: AntiHeat.enabled
        }
    ].concat(SysInfo.isLaptop ? [
        {
            id: "lidStay",
            name: qsTr("Stay awake on lid close"),
            desc: qsTr("Closing the lid neither suspends nor idles the laptop"),
            icon: "laptop_windows",
            enabled: props.lidStay
        }
    ] : [])
    // Dynamic lives in the battery popout's profile row only, on every
    // chassis — it is a power profile, not a feature mode
    readonly property int activeCount: features.filter(f => f.enabled).length

    function toggle(featureId: string): void {
        switch (featureId) {
        case "lidStay":
            setLidStay(!props.lidStay);
            break;
        case "antiHeat":
            AntiHeat.setEnabled(!AntiHeat.enabled);
            break;
        case "maxPerf":
            MaxPerf.setEnabled(!MaxPerf.enabled);
            break;
        case "dynamic":
            Dynamic.setEnabled(!Dynamic.enabled);
            break;
        }
    }

    function setLidStay(value: bool): void {
        if (value === props.lidStay)
            return;

        props.lidStay = value;
        _save();
        Toaster.toast(value ? qsTr("Stay-awake enabled") : qsTr("Stay-awake disabled"), value ? qsTr("Lid close no longer suspends the laptop") : qsTr("Lid close suspends as usual"), "laptop_windows");
    }

    function _save(): void {
        store.setText(JSON.stringify({
            lidStay: props.lidStay,
            caffeine: IdleInhibitor.enabled
        }, null, 2) + "\n");
    }

    QtObject {
        id: props

        property bool lidStay: false
    }

    // The actual lid-close mode: a logind inhibitor lock held for as long as
    // the toggle is on. Blocks the lid-switch action (suspend), plus sleep and
    // idle so nothing else powers the machine down while it runs closed.
    Process {
        command: ["systemd-inhibit", "--what=handle-lid-switch:sleep:idle", "--who=Caelestia Features", "--why=Stay-awake mode is on", "--mode=block", "sleep", "infinity"]
        running: props.lidStay
    }

    Process {
        id: ensureStateDir

        command: ["mkdir", "-p", Paths.state]
    }

    FileView {
        id: store

        path: root.statePath
        watchChanges: true
        printErrors: false

        onLoaded: {
            let saved;
            try {
                saved = JSON.parse(text());
            } catch (e) {
                return;
            }
            props.lidStay = saved.lidStay ?? false;
            IdleInhibitor.enabled = saved.caffeine ?? IdleInhibitor.enabled;
        }
        onFileChanged: reload()
    }

    Component.onCompleted: ensureStateDir.running = true

    IpcHandler {
        target: "features"

        function toggleMenu(): void { root.menuOpen = !root.menuOpen; }
        // Generic, for a front end that has the list from `status` and wants
        // to flip one by id — the Tauri bar's battery popout does exactly
        // that, and a named function per mode would need one added here every
        // time a mode is.
        function toggle(id: string): void { root.toggle(id); }
        function toggleLidStay(): void { root.setLidStay(!props.lidStay); }
        function toggleMaxPerf(): void { MaxPerf.setEnabled(!MaxPerf.enabled); }
        function toggleAntiHeat(): void { AntiHeat.setEnabled(!AntiHeat.enabled); }
        function status(): string {
            return root.features.map(f => `${f.id}: ${f.enabled ? "on" : "off"}`).join("; ");
        }
    }
}
