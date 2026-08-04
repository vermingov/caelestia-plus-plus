pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io

// Debug console backend, window opened by 10 rapid clicks on the bar clock
// or `qs -c caelestia ipc call debug toggle`. Reads this instance's own log
// through two feeds, both emitting `qs`'s standard timestamped line format:
//
//   sparse  `tail -F <rundir>/log.log` — the plain-text mirror of what the
//           shell would print on stdout (info and up). Runs for the whole
//           shell lifetime, so warnings and errors are still counted while
//           the window is closed.
//   debug   `qs log -f --pid <self>` filtered to debug level only, i.e. the
//           part the sparse log does not carry. Runs only while the window
//           is open with verbose on. The two feeds never overlap, so nothing
//           is counted twice.
//
// The debug feed is deliberately not resident. `qs log -f` aborts the whole
// process when its reader wakes on a partially written record: it mistakes
// the torn tail for corruption (logging.cpp continueReading), exits the event
// loop, and ~LogFollower then destroys an FcntlWaitThread still blocked in
// fcntl(F_SETLKW) on the writer's lock — QThread destroyed while running is
// qFatal. Tailing the debug firehose all session hit that every few hours and
// left a coredump each time. Keeping it to "while someone is watching" caps
// the exposure, and the sparse feed that does run all session cannot hit it.
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

    // Debug level only — info and up already arrive on the sparse feed. pipewire
    // (loop iterations, link/node churn) and dbus property sync log constantly
    // at debug level and drown everything else, so they stay muted.
    readonly property string debugRules: "*.debug=true;*.info=false;*.warning=false;*.critical=false;quickshell.service.pipewire.debug=false;quickshell.service.pipewire.*.debug=false;quickshell.dbus.properties.debug=false"

    // Plain-text mirror of this instance's log, written by quickshell next to
    // the encoded log.qslog in its run directory
    readonly property string sparseLogPath: `${Quickshell.env("XDG_RUNTIME_DIR")}/quickshell/by-pid/${Quickshell.processId}/log.log`

    onVerboseChanged: _syncDebugFeed()
    onOpenChanged: _syncDebugFeed()
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

    // Both feeds emit quickshell's timestamped line format:
    //   "2026-08-05 01:19:13.438  WARN some.category: message"
    // The category is omitted for the default one, and a message body with
    // embedded newlines continues on unprefixed lines.
    readonly property var _lineRe: /^(\d{4}-\d\d-\d\d (\d\d:\d\d:\d\d)\.\d{3})\s+(DEBUG|INFO|WARN|ERROR|CRITICAL|FATAL)(?: ([\w.]+))?: ?([\s\S]*)$/
    // The `quickshell.bare` category prints its body straight after the
    // timestamp with no level or separator
    readonly property var _bareRe: /^\d{4}-\d\d-\d\d (\d\d:\d\d:\d\d)\.\d{3}([\s\S]*)$/

    // Newest full timestamp taken off the debug feed, "yyyy-MM-dd hh:mm:ss.zzz"
    // and so orderable as a plain string
    property string _debugWatermark: ""

    function _append(raw: string, fromDebugFeed: bool): void {
        const m = _lineRe.exec(raw);
        const level = m ? m[3].toLowerCase() : "info";

        if (fromDebugFeed) {
            // Every start of the feed replays history, and a revive replays
            // what the buffer already holds — the watermark is what tells the
            // two apart
            if (m) {
                if (m[1] <= _debugWatermark)
                    return;
                _debugWatermark = m[1];
            }
        } else if (level === "debug" && debugTail.running) {
            // A shell launched with -v mirrors debug into the sparse log too,
            // and the debug feed is already carrying those lines
            return;
        }

        // Anything left is either a bare line or a continuation of a multiline
        // body, which passes through as-is
        const bare = m ? null : _bareRe.exec(raw);
        const entry = {
            time: m ? m[2] : bare ? bare[1] : Qt.formatTime(new Date(), "hh:mm:ss"),
            level: level === "critical" || level === "fatal" ? "error" : level,
            category: m ? m[4] || "" : "",
            message: m ? m[5] : bare ? bare[2] : raw
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

    // Resident feed. Only a revive replays history, and there it must not, or
    // the buffer gains a second copy of everything it already holds.
    function _startSparseFeed(history: int): void {
        sparseTail.command = ["tail", "-n", `${history}`, "-F", "--", sparseLogPath];
        sparseTail.running = true;
    }

    // On-demand feed, alive only while the window is showing debug output — see
    // the note at the top of this file for why it is not resident.
    //
    // -t is not just an opening history size: `qs log` re-applies it as a cap
    // on every wake-up while following, so a small value throws away all but
    // the last line of each batch. 500 is comfortably above any one batch, so
    // it costs a replay on start (the watermark absorbs it) and loses nothing.
    function _syncDebugFeed(): void {
        const wanted = open && verbose;
        if (wanted === debugTail.running)
            return;

        debugRevive.stop();
        if (wanted) {
            debugTail.command = ["qs", "--no-color", "--log-times", "log", "-f", "-t", "500", "--pid", `${Quickshell.processId}`, "-r", debugRules];
            debugTail.running = true;
        } else {
            _debugStopping = true;
            debugTail.running = false;
        }
    }

    property bool _debugStopping: false

    Component.onCompleted: _startSparseFeed(200)

    Process {
        id: sparseTail

        stdout: SplitParser {
            onRead: data => root._append(data, false)
        }
        // `tail -F` sits on the file across truncation and never exits on its
        // own, but the session's warning and error counts hang off this feed,
        // so a death still has to be recoverable
        onExited: {
            console.warn("caelestia.debugconsole: sparse log tail died, restarting in 2s");
            sparseRevive.restart();
        }
    }

    Process {
        id: debugTail

        stdout: SplitParser {
            onRead: data => root._append(data, true)
        }
        // Upstream `qs log -f` aborts on a torn read of the live log; that is
        // rare now that this only runs while the window is open, but a silent
        // dead feed for the rest of the session would still be worse
        onExited: {
            if (root._debugStopping) {
                root._debugStopping = false;
                return;
            }
            console.warn("caelestia.debugconsole: debug feed died, restarting in 2s");
            debugRevive.restart();
        }
    }

    Timer {
        id: sparseRevive

        interval: 2000
        onTriggered: root._startSparseFeed(0)
    }

    Timer {
        id: debugRevive

        interval: 2000
        onTriggered: root._syncDebugFeed()
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
