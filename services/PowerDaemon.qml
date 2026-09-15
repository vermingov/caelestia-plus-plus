pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Caelestia
import qs.services

// The one place that talks to power-profiles-daemon. Setting a profile goes
// through the powerprofilesctl CLI rather than quickshell's built-in
// PowerProfiles singleton: that C++ service initialises once, and if the
// daemon is slow or unreachable at that moment (boot race, daemon restart) it
// logs "will not work" and stays dead for the whole session — profile
// switching silently breaks until the shell is reloaded. It also mis-detects
// performance as unavailable on this machine. Re-probing from scratch every
// 30 s recovers from anything.
//
// Reading, though, is a D-Bus property read and nothing more. The CLI is
// Python: 150 ms of interpreter start every 30 s, forever, to learn a string
// that busctl fetches in 2.5 ms. Same daemon, same answer, same failure mode
// (no reply means down, which is what the retry path below is for).
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
                // Spaced, not immediate: a rejected switch is usually waiting
                // on a root-side knob that a systemd path unit is still
                // applying, which lands about a second later
                retryTimer.restart();
            } else {
                if (wanted && parsed.active === wanted && wasDown)
                    Toaster.toast(qsTr("Power daemon reconnected"), qsTr("%1 profile re-applied").arg(root.label(wanted)), "bolt");
                wanted = "";
                wasDown = false;
            }
            pollTimer.restart();
        }

        // busctl prints one line per property in the order asked for, each
        // as `s "value"`. A daemon that is down answers on stderr and leaves
        // stdout empty, which parses to no active profile — exactly what the
        // retry path treats as unreachable.
        function parse(text: string): var {
            const values = [];
            for (const line of text.split("\n")) {
                const m = /^\s*s\s+"(.*)"\s*$/.exec(line);
                if (m)
                    values.push(m[1]);
            }
            return {
                active: values[0] ?? "",
                degradation: values[1] ?? ""
            };
        }
    }

    Process {
        id: probe

        command: ["busctl", "--system", "get-property", "org.freedesktop.UPower.PowerProfiles", "/org/freedesktop/UPower/PowerProfiles", "org.freedesktop.UPower.PowerProfiles", "ActiveProfile", "PerformanceDegraded"]
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
        id: retryTimer

        interval: 1200
        onTriggered: {
            if (internal.wanted)
                internal.apply(internal.wanted);
        }
    }

    Timer {
        id: pollTimer

        interval: internal.ready ? 30000 : internal.retryMs
        onTriggered: probe.running = true
    }

    Component.onCompleted: probe.running = true
}
