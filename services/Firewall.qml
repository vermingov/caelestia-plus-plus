pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Bridge to redwalld over its Unix socket. Newline-delimited JSON:
//   daemon -> us:  {"t":"rules",...}  {"t":"ask",...}
//   us -> daemon:  {"t":"verdict"|"setrule"|"delrule"|"getrules"|"simconnect",...}
// Auto-reconnects, so the bar recovers whenever the daemon (re)starts.
Singleton {
    id: root

    readonly property string sockPath: "/run/redwall/ui.sock"
    readonly property bool connected: sockLoader.item?.connected ?? false

    // Apps waiting on a verdict right now (drives the prompt).
    property var pending: []
    // Every remembered rule (drives the management panel).
    property var rules: []
    readonly property int pendingCount: pending.length

    // Master on/off (persisted daemon-side). When off, all traffic passes but
    // rules are kept.
    property bool enabled: true

    // Toggled by the bar shield; opens the rules panel.
    property bool panelOpen: false

    function _send(obj: var): void {
        if (sockLoader.item?.connected)
            sockLoader.item.write(JSON.stringify(obj) + "\n");
    }

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
    function deny(id: int): void { verdict(id, "deny", true); }
    function allowOnce(id: int): void { verdict(id, "allow", false); }

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
            // Drop any prompt whose app just gained a rule (from any source).
            const ruled = new Set(root.rules.map(r => r.exe));
            root.pending = root.pending.filter(p => !ruled.has(p.exe));
        } else if (msg.t === "ask") {
            if (!root.pending.some(p => p.id === msg.id))
                root.pending = [...root.pending, msg];
        } else if (msg.t === "resolved") {
            root.pending = root.pending.filter(p => p.exe !== msg.exe);
        } else if (msg.t === "state") {
            root.enabled = msg.enabled ?? true;
        }
    }

    // A Quickshell Socket does not re-attempt after a failed connect (toggling
    // `connected` on the existing object is a no-op), so a daemon that starts
    // after the shell would never attach. The socket lives in a Loader that is
    // rebuilt from scratch every couple of seconds until the fresh QLocalSocket
    // connects; once connected the reconnect timer stops. The sourceComponent
    // MUST be an explicit Component — an inline Socket isn't recreated on
    // active toggling.
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

    // Lets a keybind / `qs -c caelestia ipc call firewall togglePanel` open the
    // rules manager without the bar.
    IpcHandler {
        target: "firewall"

        function togglePanel(): void { root.panelOpen = !root.panelOpen; }
        function openPanel(): void { root.panelOpen = true; }
        function closePanel(): void { root.panelOpen = false; }
        function status(): string {
            return root.connected ? `connected; ${root.pendingCount} pending; ${root.rules.length} rules` : "daemon offline";
        }
    }
}
