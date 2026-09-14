pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import qs.services

// Update channel for the Caelestia++ fork: compares the running shell's
// checkout against origin/main on GitHub and can fast-forward + restart.
// The shell dir is a plain git clone, so "update" is just a pull — package
// files (plugin, CLI) are versioned separately and unaffected.
Singleton {
    id: root

    readonly property string repoDir: Quickshell.shellDir
    readonly property string remoteBranch: "origin/main"
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
    property list<string> changelog
    property string lastChecked
    property string lastError

    readonly property bool updateAvailable: commitsBehind > 0

    // Auto-apply updates once per session, at startup only (never yanked out
    // from under an active session by a periodic check). Default on so fixes
    // actually reach people; opt out by flipping this in the Updates tab.
    property alias autoUpdate: props.autoUpdate
    property bool _startupPass: true

    PersistentProperties {
        id: props

        property bool autoUpdate: true

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
            git fetch --quiet origin main || exit 2
            echo "@head $(git rev-parse --short HEAD)"
            echo "@behind $(git rev-list --count HEAD..${root.remoteBranch})"
            git log --format=%s "HEAD..${root.remoteBranch}"`]

        stdout: StdioCollector {
            onStreamFinished: {
                const lines = text.trim().split("\n").filter(l => l);
                const head = lines.find(l => l.startsWith("@head "));
                const behind = lines.find(l => l.startsWith("@behind "));
                root.headCommit = head ? head.slice(6) : "";
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
                Toaster.toast(qsTr("Updating Caelestia++"), qsTr("%1 new change(s) — applying and restarting").arg(root.commitsBehind), "update");
                root.update();
            }
        }
    }

    Process {
        id: updateProc

        command: ["sh", "-c", `set -e
cd '${root.repoDir}'
url=$(git remote get-url origin) || exit 3
[ "$url" = '${root.expectedRemote}' ] || { echo "REMOTE_MISMATCH:$url"; exit 4; }
case "$url" in https://*) ;; *) echo INSECURE_REMOTE; exit 5 ;; esac
git fetch --quiet origin main || exit 2
newtip=$(git rev-parse --verify origin/main) || exit 2
signers="$HOME/.config/caelestia/update-allowed-signers"
if [ -f "$signers" ]; then
    git -c gpg.ssh.allowedSignersFile="$signers" verify-commit "$newtip" 2>/dev/null || { echo BAD_SIGNATURE; exit 6; }
fi
git merge --ff-only "$newtip" || exit 7
# Never restart into a quickshell that cannot start (Qt moved on under it)
qs --version >/dev/null 2>&1 || { echo QS_BROKEN; exit 8; }`]

        onExited: code => {
            root.updating = false;
            if (code !== 0) {
                root.lastError =
                    code === 4 ? qsTr("Update blocked: origin is not the expected Caelestia++ remote") :
                    code === 5 ? qsTr("Update blocked: the update remote is not HTTPS") :
                    code === 6 ? qsTr("Update blocked: the new commit is not signed by a trusted key") :
                    code === 2 ? qsTr("Could not reach the update server") :
                    code === 8 ? qsTr("Updated, but quickshell can no longer start against this Qt — rebuild it from System check before restarting") :
                    qsTr("Update failed — local changes may conflict");
                return;
            }
            // Relaunch outside our own process tree so the new checkout loads
            Quickshell.execDetached(["sh", "-c", "sleep 0.3; pkill -x qs; sleep 1; caelestia shell -d"]);
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
