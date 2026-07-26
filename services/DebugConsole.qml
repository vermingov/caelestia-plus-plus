pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Debug console backend, window opened by 10 rapid clicks on the bar clock
// or `qs -c caelestia ipc call debug toggle`. Tails this instance's own log
// through `qs log -f --pid <self>` for the whole shell lifetime: quickshell
// records every category to the log file regardless of stdout filtering, so
// full debug output is available without relaunching the shell in verbose
// mode, and warnings/errors are captured even while the window is closed.
Singleton {
    id: root

    property bool open: false
    property string panelTab: "console" // console | scan
    property bool paused: false
    property bool verbose: true

    // View filters; the panel binds these and renders `lines`
    property string levelFilter: "all" // all | debug | info | warn | error
    property string query: ""

    readonly property int maxLines: 1500
    property int warnCount: 0
    property int errorCount: 0
    property string memUsage: "..."

    // Filtered view of `buffer`, capped at maxLines
    readonly property ListModel lines: ListModel {}

    // Rolling capture, [{time, level, category, message}]
    readonly property var buffer: []

    // Session-long dedup of warnings and errors, keyed by category+message.
    // Survives buffer overflow and clear(), feeds copyDiagnostics(). Capped:
    // plenty of messages embed a pid, path or value, so the number of distinct
    // keys is effectively unbounded over a long session.
    readonly property int maxIssues: 750
    readonly property var issues: []
    readonly property var _issueIndex: ({})

    // The panel renders `lines` as one selectable text document, so it needs
    // to know about single appends (cheap) vs anything else (full rebuild)
    signal lineAppended(entry: var)
    signal viewReset

    // pipewire (loop iterations, link/node churn) and dbus property sync log
    // constantly at debug level; they drown everything else AND the sheer
    // read volume can crash `qs log`'s reader thread, so verbose mutes them
    readonly property string verboseRules: "*.debug=true;quickshell.service.pipewire.debug=false;quickshell.service.pipewire.*.debug=false;quickshell.dbus.properties.debug=false"
    readonly property string quietRules: "*.debug=false"

    onVerboseChanged: _restartTail(false)
    onLevelFilterChanged: _refill()
    onQueryChanged: _refill()
    onPausedChanged: {
        // Capture never stops while paused, only the view does; catch up
        if (!paused)
            _refill();
    }

    function clear(): void {
        buffer.length = 0;
        lines.clear();
        viewReset();
    }

    function copyVisible(): void {
        const rows = [];
        for (let i = 0; i < lines.count; i++) {
            const l = lines.get(i);
            rows.push(`${l.time} ${l.level.toUpperCase()} ${l.category}: ${l.message}`);
        }
        Quickshell.execDetached(["wl-copy", rows.join("\n")]);
    }

    // Every distinct warning/error seen this session, duplicates collapsed
    function copyDiagnostics(): void {
        const head = [`Quickshell PID: ${Quickshell.processId}`, `Memory (RSS): ${memUsage}`, `Session: ${warnCount} warnings, ${errorCount} errors (${issues.length} distinct)`, `Screens: ${Quickshell.screens.map(s => `${s.name} ${s.width}x${s.height}@${Math.round(s.devicePixelRatio * 100) / 100}x`).join(", ")}`, ""];
        const rows = issues.map(i => `${i.level.toUpperCase()} ${i.category}: ${i.message}${i.count > 1 ? ` (x${i.count}, ${i.firstTime} - ${i.lastTime})` : ` (${i.firstTime})`}`);
        Quickshell.execDetached(["wl-copy", head.concat(rows).join("\n")]);
    }

    function _matches(entry: var): bool {
        if (levelFilter !== "all" && entry.level !== levelFilter)
            return false;
        if (query && !`${entry.category} ${entry.message}`.toLowerCase().includes(query.toLowerCase()))
            return false;
        return true;
    }

    function _refill(): void {
        lines.clear();
        for (const entry of buffer)
            if (_matches(entry))
                lines.append(entry);
        viewReset();
    }

    function _recordIssue(entry: var): void {
        const key = `${entry.level}|${entry.category}|${entry.message}`;
        const known = _issueIndex[key];
        if (known) {
            known.count++;
            known.lastTime = entry.time;
            return;
        }
        const issue = {
            key: key,
            level: entry.level,
            category: entry.category,
            message: entry.message,
            count: 1,
            firstTime: entry.time,
            lastTime: entry.time
        };
        _issueIndex[key] = issue;
        issues.push(issue);

        // Oldest distinct problems go first; the index has to shed the same
        // keys or it keeps growing after the list stops.
        if (issues.length > maxIssues) {
            for (const dropped of issues.splice(0, issues.length - maxIssues))
                delete _issueIndex[dropped.key];
        }
    }

    function _append(raw: string): void {
        // `qs log` line shape: " LEVEL category.name: message"; anything that
        // doesn't match (e.g. multiline continuations) passes through as-is
        const m = /^\s*(DEBUG|INFO|WARN|ERROR|CRITICAL|FATAL)\s+([\w.]+):\s?(.*)$/.exec(raw);
        const level = m ? m[1].toLowerCase() : "info";
        const entry = {
            time: Qt.formatTime(new Date(), "hh:mm:ss"),
            level: level === "critical" || level === "fatal" ? "error" : level,
            category: m ? m[2] : "",
            message: m ? m[3] : raw
        };

        if (entry.level === "warn") {
            warnCount++;
            _recordIssue(entry);
        } else if (entry.level === "error") {
            errorCount++;
            _recordIssue(entry);
        }

        buffer.push(entry);
        if (buffer.length > maxLines)
            buffer.splice(0, 200);

        if (!paused && _matches(entry)) {
            lines.append(entry);
            // Trim in chunks: dropping one line per append would force the
            // panel to rebuild its text document on every overflowing line
            if (lines.count > maxLines) {
                lines.remove(0, 200);
                viewReset();
            } else {
                lineAppended(entry);
            }
        }
    }

    // Runs for the shell's lifetime; restarted only on rule changes. `qs log`
    // rejects -t 0 (range is >= 1), so no-history restarts tail one line and
    // drop it on arrival to keep the buffer duplicate-free
    function _restartTail(withHistory: bool): void {
        _tailStopping = tail.running;
        _skipReplayed = withHistory ? 0 : 1;
        tail.running = false;
        tail.command = ["qs", "--no-color", "log", "-f", "-t", withHistory ? "200" : "1", "--pid", `${Quickshell.processId}`, "-r", verbose ? verboseRules : quietRules];
        tail.running = true;
    }

    property bool _tailStopping: false
    property int _skipReplayed: 0

    Component.onCompleted: _restartTail(true)

    Process {
        id: tail

        stdout: SplitParser {
            onRead: data => {
                if (root._skipReplayed > 0) {
                    root._skipReplayed--;
                    return;
                }
                root._append(data);
            }
        }
        // `qs log`'s reader thread can crash on a live log under heavy write
        // volume ("[READER] ERROR ... QThread: Destroyed"), which would end
        // the feed silently for the rest of the session — revive it instead
        onExited: {
            if (root._tailStopping) {
                root._tailStopping = false;
                return;
            }
            console.warn("caelestia.debugconsole: log tail died, restarting in 2s");
            tailRevive.restart();
        }
    }

    Timer {
        id: tailRevive

        interval: 2000
        onTriggered: root._restartTail(false)
    }

    Process {
        id: memProc

        command: ["ps", "-o", "rss=", "-p", `${Quickshell.processId}`]
        stdout: StdioCollector {
            onStreamFinished: {
                const kb = parseInt(text.trim(), 10);
                if (!isNaN(kb))
                    root.memUsage = `${(kb / 1024).toFixed(0)} MB`;
            }
        }
    }

    Timer {
        running: root.open
        interval: 5000
        repeat: true
        triggeredOnStart: true
        onTriggered: memProc.running = true
    }

    IpcHandler {
        target: "debug"

        function toggle(): void {
            root.open = !root.open;
        }
        // "show" is unusable as an IPC function name: it collides with the
        // CLI's `ipc show` subcommand and prints target info instead
        function openPanel(): void {
            root.open = true;
        }
        function closePanel(): void {
            root.open = false;
        }
    }
}
