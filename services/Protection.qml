pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Caelestia
import qs.utils

// Bridge to redguardd over its Unix socket. Same newline-JSON contract as the
// firewall (services/Firewall.qml): the daemon streams asks/rules/state, we
// send verdicts and rule edits. Auto-reconnects so the tab recovers whenever
// the daemon (re)starts. See system/redguard/ for the enforcement half.
Singleton {
    id: root

    readonly property string sockPath: "/run/redguard/ui.sock"
    readonly property bool connected: sockLoader.item?.connected ?? false

    // Frozen processes waiting on a verdict right now (drives the prompt).
    property var pending: []
    // Every remembered allow/block rule (drives the management tab).
    property var rules: []
    readonly property int pendingCount: pending.length

    // Master on/off (persisted daemon-side). When off, nothing is frozen.
    property bool enabled: true

    // Root half installs on first enable through pkexec, like the perf
    // features — no terminal needed. `connected` doubles as "installed and
    // running", so a fresh machine shows the install affordance.
    property bool installing: false

    function install(): void {
        if (root.installing)
            return;
        root.installing = true;
        Toaster.toast(qsTr("Setting up Protection"), qsTr("Installing the privileged half — enter your password"), "key");
        installer.running = true;
    }

    Process {
        id: installer

        command: ["timeout", "600", "pkexec", "bash", `${Quickshell.shellDir}/system/redguard/install.sh`]
        stdout: SplitParser {
            onRead: data => console.info("redguard install:", data)
        }
        onExited: code => {
            root.installing = false;
            if (code === 0)
                Toaster.toast(qsTr("Protection ready"), qsTr("Behavioral monitoring is now active"), "security");
            else
                Toaster.toast(qsTr("Setup not completed"), code === 126 || code === 127 ? qsTr("Authentication was dismissed — try again") : qsTr("Installer failed (code %1) — see the debug console").arg(code), "warning");
        }
    }

    function _send(obj: var): void {
        if (sockLoader.item?.connected)
            sockLoader.item.write(JSON.stringify(obj) + "\n");
    }

    // action: "allow" | "block" | "once". remember only meaningful for
    // allow/block (once is a one-shot release).
    function verdict(id: int, action: string, remember: bool): void {
        _send({
            t: "verdict",
            id: id,
            action: action,
            remember: remember
        });
        root.pending = root.pending.filter(p => p.id !== id);
    }

    function allow(id: int): void { verdict(id, "allow", true); }
    function block(id: int): void { verdict(id, "block", true); }
    function allowOnce(id: int): void { verdict(id, "once", false); }

    function setRule(exe: string, action: string, name: string): void {
        _send({
            t: "setrule",
            exe: exe,
            action: action,
            name: name
        });
    }

    function delRule(exe: string): void {
        _send({
            t: "delrule",
            exe: exe
        });
    }

    function setEnabled(value: bool): void {
        root.enabled = value;
        _send({
            t: "setenabled",
            enabled: value
        });
    }

    function _handle(line: string): void {
        if (!line)
            return;
        let msg;
        try {
            msg = JSON.parse(line);
        } catch (e) {
            return;
        }
        if (msg.t === "rules") {
            root.rules = msg.rules ?? [];
        } else if (msg.t === "ask") {
            if (!root.pending.some(p => p.id === msg.id))
                root.pending = [...root.pending, msg];
        } else if (msg.t === "resolved") {
            root.pending = root.pending.filter(p => p.id !== msg.id);
        } else if (msg.t === "state") {
            root.enabled = msg.enabled ?? true;
        } else if (msg.t === "event") {
            // Silent daemon actions must never be invisible to the user:
            // auto-blocks from remembered rules and timeout releases both
            // surface as toasts (there is no prompt for either).
            if (msg.kind === "blocked")
                Toaster.toast(qsTr("Protection blocked %1").arg(msg.name ?? "a process"), msg.detail ?? "", "gpp_bad");
            else if (msg.kind === "timeout-release")
                Toaster.toast(qsTr("%1 released without a verdict").arg(msg.name ?? "A frozen process"), msg.detail ?? "", "history_toggle_off");
            else if (msg.kind === "unmonitored")
                Toaster.toast(qsTr("Suspicious %1 released").arg(msg.name ?? "process"), qsTr("Detected while no UI was connected — review the rules tab"), "gpp_maybe");
        }
    }

    // A Quickshell Socket does not reliably re-attempt after a failed connect
    // (toggling `connected` on the existing object is a no-op), so a daemon
    // that starts AFTER the shell — e.g. installing Protection while it runs —
    // would never attach. Instead the socket lives in a Loader that is rebuilt
    // from scratch every couple of seconds until the fresh QLocalSocket
    // connects; once connected the reconnect timer stops.
    Loader {
        id: sockLoader

        active: true
        sourceComponent: Component {
            Socket {
                path: root.sockPath
                connected: true

                parser: SplitParser {
                    splitMarker: "\n"
                    onRead: line => root._handle(line)
                }

                onConnectionStateChanged: {
                    if (!connected)
                        root.pending = [];
                }
            }
        }
    }

    // Retry fast right after start (the daemon may still be coming up), then
    // back off: an install without the daemon must not rebuild the socket
    // every two seconds for the shell's lifetime
    Timer {
        id: reconnectTimer

        property int backoffMs: 2000

        interval: backoffMs
        running: !root.connected
        repeat: true
        onRunningChanged: {
            if (running)
                backoffMs = 2000;
        }
        onTriggered: {
            backoffMs = Math.min(backoffMs * 2, 30000);
            sockLoader.active = false;
            reconnectKick.restart();
        }
    }

    Timer {
        id: reconnectKick
        interval: 50
        onTriggered: sockLoader.active = true
    }

    IpcHandler {
        target: "protection"

        function status(): string {
            return root.connected ? `connected; ${root.pendingCount} frozen; ${root.rules.length} rules` : "daemon offline";
        }
    }
}
