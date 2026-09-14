pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import qs.services

// Startup Apps backend for the security center. Two real, safe, universal
// sources — XDG autostart entries (via assets/startup-ctl.py, with proper
// user-shadows-system masking) and enabled systemd --user services. Hyprland
// exec-once lines are not managed here: this setup's Hyprland config is Lua,
// so editing it programmatically is unsafe.
Singleton {
    id: root

    // [{key, name, exec, icon, source: "autostart"|"systemd", enabled, path}]
    property var entries: []
    property bool scanning: false

    // Either the Rust binary or the script it replaced; see Helpers
    readonly property var helper: Helpers.command("startup")

    // Parsed as scans complete; merged so a stale half never blanks the list.
    property var _autostart: []
    property var _systemd: []

    function refresh(): void {
        scanning = true;
        _autostart = [];
        _systemd = [];
        autostartScan.running = true;
        systemdScan.running = true;
    }

    function _merge(): void {
        entries = [..._autostart, ..._systemd].sort((a, b) => a.name.localeCompare(b.name));
        scanning = false;
    }

    function toggle(entry: var): void {
        if (entry.source === "systemd") {
            systemctl.exec(["systemctl", "--user", entry.enabled ? "disable" : "enable", entry.key]);
        } else {
            Quickshell.execDetached([...root.helper, "set-enabled", entry.path, entry.enabled ? "0" : "1"]);
        }
        _bumpLater();
    }

    function remove(entry: var): void {
        if (entry.source === "systemd")
            systemctl.exec(["systemctl", "--user", "disable", entry.key]);
        else
            Quickshell.execDetached([...root.helper, "remove", entry.path]);
        _bumpLater();
    }

    function add(name: string, exe: string): void {
        if (!name || !exe)
            return;
        Quickshell.execDetached([...root.helper, "add", name, exe]);
        _bumpLater();
    }

    // File/enable ops are fire-and-forget; re-scan shortly after so the list
    // reflects the change without racing the write.
    function _bumpLater(): void {
        rescanTimer.restart();
    }

    Timer {
        id: rescanTimer
        interval: 400
        onTriggered: root.refresh()
    }

    Process {
        id: autostartScan

        command: [...root.helper, "scan"]
        stdout: StdioCollector {
            onStreamFinished: {
                const rows = [];
                for (const line of text.trim().split("\n")) {
                    if (!line)
                        continue;
                    const f = line.split("|");
                    if (f[0] !== "as")
                        continue;
                    rows.push({
                        key: f[2].split("/").pop(),
                        source: "autostart",
                        enabled: f[1] === "1",
                        path: f[2],
                        name: f[3] || f[2].split("/").pop(),
                        exec: f[4] ?? "",
                        icon: f[5] ?? ""
                    });
                }
                root._autostart = rows;
                if (!systemdScan.running)
                    root._merge();
            }
        }
    }

    // Enabled user services only. Sockets/targets/timers are infrastructure,
    // not "apps", so they are left out of this view.
    Process {
        id: systemdScan

        command: ["systemctl", "--user", "list-unit-files", "--type=service", "--state=enabled", "--no-legend", "--plain"]
        stdout: StdioCollector {
            onStreamFinished: {
                const rows = [];
                for (const line of text.trim().split("\n")) {
                    if (!line)
                        continue;
                    const unit = line.split(/\s+/)[0];
                    if (!unit || !unit.endsWith(".service"))
                        continue;
                    rows.push({
                        key: unit,
                        source: "systemd",
                        enabled: true,
                        path: unit,
                        name: unit.replace(/\.service$/, ""),
                        exec: qsTr("systemd user service"),
                        icon: ""
                    });
                }
                root._systemd = rows;
                if (!autostartScan.running)
                    root._merge();
            }
        }
    }

    Process {
        id: systemctl
        function exec(cmd: var): void {
            command = cmd;
            running = true;
        }
    }

    IpcHandler {
        target: "startup"

        function refresh(): void { root.refresh(); }
        function status(): string {
            return `${root.entries.length} startup entries`;
        }
    }
}
