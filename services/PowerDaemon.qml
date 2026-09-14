pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import qs.services

// The one place that talks to power-profiles-daemon. Everything goes through
// the powerprofilesctl CLI instead of quickshell's built-in PowerProfiles
// singleton: that C++ service initialises once, and if the daemon is slow or
// unreachable at that moment (boot race, daemon restart) it logs "will not
// work" and stays dead for the whole session — profile switching silently
// breaks until the shell is reloaded. It also mis-detects performance as
// unavailable on this machine. A 30 s CLI poll (immediate after our own
// set) costs a short python subprocess and recovers from anything.
//
// A profile set while the daemon is down is remembered and applied the moment
// it comes back, so a mode picked right after boot sticks.
Singleton {
    id: root

    readonly property bool ready: internal.ready
    // "power-saver" | "balanced" | "performance"
    readonly property string profile: internal.profile
    // Empty when performance is not degraded, otherwise ppd's reason string
    readonly property string degradation: internal.degradation
    readonly property string profileLabel: label(internal.profile)

    function label(p: string): string {
        if (p === "power-saver")
            return qsTr("Power Saver");
        if (p === "performance")
            return qsTr("Performance");
        return qsTr("Balanced");
    }

    function setProfile(p: string): void {
        internal.wanted = p;
        internal.applyTries = 0;
        internal.profile = p; // optimistic; the next probe confirms or corrects
        internal.apply(p);
    }

    QtObject {
        id: internal

        property bool ready: false
        property string profile: "balanced"
        property string degradation: ""

        // Last profile requested through the shell; kept until a probe
        // confirms the daemon has it, so it survives a daemon outage
        property string wanted: ""
        property int applyTries: 0

        // While the daemon is down, retry with backoff; once up, poll slowly
        // to track external changes and notice the daemon dying
        property int retryMs: 2000
        property bool wasDown: false

        function apply(p: string): void {
            setProc.command = ["powerprofilesctl", "set", p];
            setProc.running = true;
        }

        function onProbe(text: string): void {
            const parsed = parse(text);
            if (!parsed.active) {
                if (ready)
                    console.warn("caelestia.powerdaemon: power-profiles-daemon unreachable, retrying");
                ready = false;
                wasDown = true;
                retryMs = Math.min(retryMs * 2, 15000);
                pollTimer.restart();
                return;
            }

            ready = true;
            retryMs = 2000;
            profile = parsed.active;
            degradation = parsed.degradation;

            if (wanted && parsed.active !== wanted && applyTries < 3) {
                applyTries++;
                console.info(`caelestia.powerdaemon: re-applying ${wanted} (daemon has ${parsed.active})`);
                apply(wanted);
            } else {
                if (wanted && parsed.active === wanted && wasDown)
                    Toaster.toast(qsTr("Power daemon reconnected"), qsTr("%1 profile re-applied").arg(root.label(wanted)), "bolt");
                wanted = "";
                wasDown = false;
            }
            pollTimer.restart();
        }

        // powerprofilesctl lists sections as "* balanced:" (active) and
        // "  performance:", with "Degraded:   no|<reason>" attribute lines
        function parse(text: string): var {
            let active = "", degradation = "", section = "";
            for (const line of text.split("\n")) {
                const head = /^\s*(\*?)\s*([a-z-]+):\s*$/.exec(line);
                if (head) {
                    section = head[2];
                    if (head[1] === "*")
                        active = section;
                    continue;
                }
                const deg = /^\s+Degraded:\s+(.*\S)\s*$/.exec(line);
                if (deg && deg[1] !== "no")
                    degradation = deg[1];
            }
            return {active, degradation};
        }
    }

    Process {
        id: probe

        command: ["powerprofilesctl"]
        stdout: StdioCollector {
            // A down daemon (or missing tool) leaves stdout empty, which
            // parses to no active profile — exit codes aren't needed
            onStreamFinished: internal.onProbe(text)
        }
    }

    Process {
        id: setProc

        onExited: probe.running = true
    }

    Timer {
        id: pollTimer

        interval: internal.ready ? 30000 : internal.retryMs
        onTriggered: probe.running = true
    }

    Component.onCompleted: probe.running = true
}
