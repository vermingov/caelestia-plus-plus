pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import qs.services

// Shared state + overall security posture for the security center
// (modules/protection/SecurityCenter.qml), the tabbed panel behind the bar
// shield that hosts an Overview dashboard plus Protection, Firewall and
// Startup Apps. The data backends live in their own singletons
// (Protection, Firewall, Startup); this tracks which panel is showing and
// rolls their states into one at-a-glance posture.
Singleton {
    id: root

    // overview | protection | firewall | startup
    property string tab: "overview"
    property bool panelOpen: false

    // Prompts waiting for a decision across both guards.
    readonly property int alertCount: Protection.pendingCount + Firewall.pendingCount

    // 0 protected · 1 attention · 2 alert — drives the hero and the bar shield.
    readonly property int posture: {
        if (alertCount > 0)
            return 2;
        const protOk = Protection.connected && Protection.enabled;
        const fwOk = Firewall.connected && Firewall.enabled;
        return protOk && fwOk ? 0 : 1;
    }

    function postureTitle(): string {
        switch (posture) {
        case 2:
            return qsTr("Action needed");
        case 1:
            return qsTr("Review recommended");
        default:
            return qsTr("Protected");
        }
    }

    function postureSubtitle(): string {
        if (posture === 2)
            return alertCount === 1 ? qsTr("1 prompt is waiting for your decision") : qsTr("%1 prompts are waiting for your decision").arg(alertCount);
        const guard = (connected, enabled, name) => connected ? (enabled ? qsTr("%1 on").arg(name) : qsTr("%1 off").arg(name)) : qsTr("%1 not set up").arg(name);
        return `${guard(Protection.connected, Protection.enabled, qsTr("Protection"))} · ${guard(Firewall.connected, Firewall.enabled, qsTr("Firewall"))}`;
    }

    // Opens it on a page, in the external bar where that is the one drawing
    // it and here otherwise.
    function open(which: string): void {
        if (ExternalBar.hasSecurity) {
            ExternalBar.ask(["security", which || root.tab]);
            return;
        }
        if (which)
            root.tab = which;
        root.panelOpen = true;
    }

    IpcHandler {
        target: "security"

        // NB: never name an IPC function "show" — it collides with the qs CLI
        // `ipc show` subcommand and never invokes.
        function togglePanel(): void {
            if (ExternalBar.hasSecurity)
                root.open("");
            else
                root.panelOpen = !root.panelOpen;
        }
        function openPanel(): void { root.open(""); }
        function closePanel(): void { root.panelOpen = false; }
        function openTab(which: string): void { root.open(which); }
    }
}
