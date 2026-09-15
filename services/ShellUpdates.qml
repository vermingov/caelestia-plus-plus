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

        command: ["sh", "-c", `set -e
cd '${root.repoDir}'
url=$(git remote get-url origin) || exit 3
[ "$url" = '${root.expectedRemote}' ] || { echo "REMOTE_MISMATCH:$url"; exit 4; }
case "$url" in https://*) ;; *) echo INSECURE_REMOTE; exit 5 ;; esac
git fetch --quiet --tags --prune --prune-tags origin || exit 2
tag=$(git tag --list '${root.releaseTagGlob}' --sort=-v:refname | head -1)
[ -n "$tag" ] || { echo NO_RELEASE; exit 9; }
newtip=$(git rev-parse --verify "$tag^{commit}") || exit 2
signers="$HOME/.config/caelestia/update-allowed-signers"
if [ -f "$signers" ]; then
    git -c gpg.ssh.allowedSignersFile="$signers" verify-commit "$newtip" 2>/dev/null || { echo BAD_SIGNATURE; exit 6; }
fi
git merge --ff-only "$newtip" || exit 7
# The checkout has moved; the installed binaries have not. A release that
# adds a helper would otherwise restart into QML calling a tool the old
# binary has never heard of. A build that fails is not fatal — the Python
# fallbacks are still there and still correct.
if command -v cargo >/dev/null 2>&1; then
    for d in cli tools; do
        [ -x "$d/install.sh" ] && "$d/install.sh" >/dev/null 2>&1 || true
    done
fi
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
                    code === 9 ? qsTr("No published release to update to") :
                    qsTr("Update failed — local changes may conflict");
                return;
            }
            // Relaunch outside our own process tree so the new checkout loads
            Quickshell.execDetached(["sh", "-c", "sleep 0.3; pkill -x qs; sleep 1; caelestia shell -d"]);
        }
    }


    // Builds the bar to match the checkout, when the checkout has moved ahead
    // of what is installed.
    //
    // The updater cannot do this itself. The update script that runs is the
    // one in the checkout being replaced, so a release can never teach an
    // older shell how to install something new — which is exactly how v2.5.0
    // shipped a bar that nobody's updater knew to build. Doing it here, from
    // the version that has just started, is the only place a release can
    // reach.
    //
    // Keyed on the commit the binaries were built from, so it is a no-op on
    // every startup but the first after an update.
    function provisionApps(): void {
        if (installingApps || updating || !autoInstallApps)
            return;
        installingApps = true;
        provisionProc.running = true;
    }

    Process {
        id: provisionProc

        command: ["sh", "-c", `cd '${root.repoDir}' || exit 0
[ -x bar/install.sh ] || exit 0
# Someone who removed the bar on purpose gets to keep it removed.
[ -f '${Paths.state}/bar-optout' ] && exit 0
command -v cargo >/dev/null 2>&1 || exit 0
command -v npm >/dev/null 2>&1 || exit 0
head=$(git rev-parse HEAD 2>/dev/null) || exit 0
stamp='${Paths.state}/bar-built-from'
[ "$(cat "$stamp" 2>/dev/null)" = "$head" ] && exit 0
mkdir -p '${Paths.state}'
bar/install.sh >/dev/null 2>&1 || exit 1
printf '%s' "$head" > "$stamp"
echo BUILT`]

        stdout: StdioCollector {
            onStreamFinished: root._built = text.includes("BUILT")
        }

        onExited: code => {
            root.installingApps = false;
            if (code !== 0) {
                // Not an error worth a toast: the shell's own bar is still
                // there and still correct, which is what they were using a
                // moment ago anyway.
                root.lastError = qsTr("Could not build the bar — the shell's own is still in use");
                return;
            }
            if (!root._built)
                return;
            // Both singletons learned whether these existed at startup, and
            // they have just stopped being right.
            ExternalBar.recheck();
            Launcher.recheck();
            Toaster.toast(qsTr("Bar installed"), qsTr("The Caelestia++ bar and launcher are now in use"), "update");
        }
    }

    property bool _built

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
