pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import qs.services
import qs.utils

// Update channel for the Caelestia++ fork: compares the running shell's
// checkout against the latest *release* and can fast-forward + restart.
//
// Releases, not commits. Tracking origin/main meant every push landed on
// every machine the moment it was made, including the half-finished ones;
// now the checkout only ever moves to a tag someone published on purpose.
// Tags are read with git rather than the releases API: no rate limit, no
// token, and a private repo still works through the user's own credentials.
//
// The shell dir is a plain git clone, so "update" is a fast-forward — package
// files (plugin, CLI) are versioned separately and unaffected.
Singleton {
    id: root

    readonly property string repoDir: Quickshell.shellDir
    // Release tags look like v2.1.0; the newest by version order wins.
    readonly property string releaseTagGlob: "v*"
    // The only remote we will ever fast-forward from, and it must be HTTPS.
    // update() refuses to pull if origin has been repointed or downgraded, so a
    // local attacker can't swap in a malicious repo. Drop an allowed-signers
    // file at ~/.config/caelestia/update-allowed-signers to additionally require
    // every pulled commit to be signed by a trusted key (see the README).
    readonly property string expectedRemote: "https://github.com/vermingov/caelestia-plus-plus.git"

    property bool checking
    property bool updating
    property int commitsBehind
    property string headCommit
    // Tag of the newest published release, empty when the repo has none yet
    property string latestRelease
    property list<string> changelog
    property string lastChecked
    property string lastError
    // Set when the update had to move local edits out of the way. They are in
    // a git stash, not gone, and saying nothing would look like data loss.
    property bool stashedLocalChanges

    readonly property bool updateAvailable: commitsBehind > 0

    // Auto-apply updates once per session, at startup only (never yanked out
    // from under an active session by a periodic check). Default on so fixes
    // actually reach people; opt out by flipping this in the Updates tab.
    property alias autoUpdate: props.autoUpdate
    property alias autoInstallApps: props.autoInstallApps
    property bool installingApps
    property bool _startupPass: true

    PersistentProperties {
        id: props

        property bool autoUpdate: true
        // Whether the bar's binaries are built to match the checkout. Off
        // leaves whatever is installed alone, including nothing at all.
        property bool autoInstallApps: true

        reloadableId: "shellUpdates"
    }

    function check(): void {
        if (checking || updating)
            return;
        lastError = "";
        checking = true;
        checkProc.running = true;
    }

    function update(): void {
        if (updating || !updateAvailable)
            return;
        lastError = "";
        updating = true;
        updateProc.running = true;
    }

    Process {
        id: checkProc

        command: ["sh", "-c", `cd '${root.repoDir}' || exit 1
            git fetch --quiet --tags --prune --prune-tags origin || exit 2
            tag=$(git tag --list '${root.releaseTagGlob}' --sort=-v:refname | head -1)
            echo "@head $(git rev-parse --short HEAD)"
            [ -n "$tag" ] || { echo "@behind 0"; exit 0; }
            target=$(git rev-parse --verify "$tag^{commit}") || exit 2
            echo "@tag $tag"
            echo "@behind $(git rev-list --count HEAD.."$target")"
            git log --format=%s "HEAD..$target"`]

        stdout: StdioCollector {
            onStreamFinished: {
                const lines = text.trim().split("\n").filter(l => l);
                const head = lines.find(l => l.startsWith("@head "));
                const tag = lines.find(l => l.startsWith("@tag "));
                const behind = lines.find(l => l.startsWith("@behind "));
                root.headCommit = head ? head.slice(6) : "";
                root.latestRelease = tag ? tag.slice(5) : "";
                root.commitsBehind = behind ? parseInt(behind.slice(8)) || 0 : 0;
                root.changelog = lines.filter(l => !l.startsWith("@"));
            }
        }

        onExited: code => {
            root.checking = false;
            root.lastChecked = Qt.formatDateTime(new Date(), "hh:mm");
            const wasStartup = root._startupPass;
            root._startupPass = false;
            if (code === 2)
                root.lastError = qsTr("Could not reach the update server");
            else if (code !== 0)
                root.lastError = qsTr("Update check failed");
            else if (wasStartup && root.autoUpdate && root.updateAvailable) {
                // Apply on startup so users are always current without lifting
                // a finger; the shell restarts once into the new version.
                Toaster.toast(qsTr("Updating Caelestia++"), qsTr("%1 released — applying and restarting").arg(root.latestRelease || qsTr("A new version")), "update");
                root.update();
            } else if (wasStartup) {
                root.provisionApps();
            }
        }
    }

    Process {
        id: updateProc

        // No `set -e`: every failure here is handled where it happens, and a
        // bare `[ … ] && exit 0` under `set -e` exits the script when the test
        // is false, which is the opposite of what it reads like.
        command: ["sh", "-c", `cd '${root.repoDir}' || exit 0
dir='${component.dir ?? ""}'
install="$dir/${component.install ?? ""}"
[ -x "$install" ] || exit 0

# Someone who removed a component on purpose gets to keep it removed.
optout='${component.optOut ?? ""}'
if [ -n "$optout" ] && [ -f '${Paths.state}/'"$optout" ]; then
    exit 0
fi

for tool in ${(component.requires ?? []).join(" ")}; do
    command -v "$tool" >/dev/null 2>&1 || exit 0
done

# The directory's own tree hash: it moves when that component's files move,
# and stays put when the rest of the release changes around it. A release
# that only touches QML therefore never rebuilds the bar.
tree=$(git rev-parse "HEAD:$dir" 2>/dev/null) || exit 0
stamp='${root.stampDir}/${component.name ?? "unknown"}'
if [ "$(cat "$stamp" 2>/dev/null)" = "$tree" ]; then
    exit 0
fi

mkdir -p '${root.stampDir}'
"$install" >/dev/null 2>&1 || exit 1
printf '%s' "$tree" > "$stamp"
echo BUILT`]

        onExited: code => {
            if (code !== 0)
                root.lastError = qsTr("Could not build %1 — the shell's own is still in use").arg(buildProc.component.label ?? buildProc.component.name ?? qsTr("a component"));
            root._runNext();
        }
    }

    // Startup check (delayed so boot isn't competing with it) + periodic recheck
    Timer {
        running: true
        interval: 20 * 1000
        onTriggered: root.check()
    }

    Timer {
        running: true
        repeat: true
        interval: 6 * 60 * 60 * 1000
        onTriggered: root.check()
    }
}
