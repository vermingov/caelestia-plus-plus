pragma Singleton

import QtQuick
import Quickshell
import Quickshell.Io
import Caelestia
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
    // Set when the update had to move local edits aside. They are in a git
    // stash, not gone, and saying nothing would look like data loss.
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
# Local edits used to fail the update outright, which stranded exactly the
# people most likely to have them. Try the clean path first, and only reach
# for the stash when git actually refuses — a tree with nothing in it is never
# touched. Untracked files come too: a release that adds a directory someone
# already has locally is how a big update fails.
if ! git merge --ff-only "$newtip" >/dev/null 2>&1; then
    if git stash push --quiet --include-untracked -m "caelestia: before $tag"; then
        if git merge --ff-only "$newtip" >/dev/null 2>&1; then
            echo STASHED
        else
            git stash pop --quiet 2>/dev/null || true
            exit 7
        fi
    else
        exit 7
    fi
fi
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

        stdout: StdioCollector {
            onStreamFinished: root.stashedLocalChanges = text.includes("STASHED")
        }

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
            if (root.stashedLocalChanges)
                Toaster.toast(qsTr("Local changes set aside"), qsTr("They are in a git stash — `git stash pop` in the shell directory brings them back"), "update");
            // Relaunch outside our own process tree so the new checkout loads
            Quickshell.execDetached(["sh", "-c", "sleep 0.3; pkill -x qs; sleep 1; caelestia shell -d"]);
        }
    }


    // Builds whatever a release says needs building, from the release's own
    // manifest rather than a list baked into this file.
    //
    // The updater cannot fix itself: the script that runs during an update is
    // the one in the checkout being replaced, so a release can never teach an
    // older shell about a directory that did not exist when that shell
    // shipped. v2.5.0 proved it by adding a bar no update could install.
    // Reading release.json out of the new checkout inverts that — the release
    // brings its own instructions, and a shell old enough to read a manifest
    // can act on components nobody had thought of yet.
    //
    // Each component is keyed on the git tree hash of its own directory, so a
    // release that only touches QML never rebuilds the bar, and one whose
    // files did change is rebuilt exactly once.
    readonly property string stampDir: `${Paths.state}/built`
    property list<var> queue
    property var job

    function provisionApps(): void {
        if (installingApps || updating || !autoInstallApps)
            return;
        manifest.reload();
    }

    function runNextComponent(): void {
        if (queue.length === 0) {
            const built = installingApps;
            installingApps = false;
            job = null;
            if (built) {
                // Both detection singletons learned what was installed at
                // startup, and that has just stopped being true.
                ExternalBar.recheck();
                Launcher.recheck();
            }
            return;
        }
        job = queue.shift();
        if (job.slow)
            Toaster.toast(qsTr("Building %1").arg(job.label ?? job.name), qsTr("A new version needs it rebuilt — this takes a few minutes"), "update");
        buildProc.running = true;
    }

    FileView {
        id: manifest

        path: `${root.repoDir}/release.json`
        printErrors: false

        // A checkout older than the manifest simply has nothing to provision.
        onLoadFailed: root.installingApps = false

        onLoaded: {
            let components = [];
            try {
                components = JSON.parse(text()).components ?? [];
            } catch (e) {
                console.warn(`ShellUpdates: release.json is not valid JSON: ${e}`);
                return;
            }
            root.queue = components.filter(c => c.name && c.dir && c.install);
            root.installingApps = root.queue.length > 0;
            root.runNextComponent();
        }
    }

    Process {
        id: buildProc

        // No `set -e`: every failure is handled where it happens, and a bare
        // `[ … ] && exit 0` under `set -e` exits when the test is false, which
        // is the opposite of what it reads like.
        command: ["sh", "-c", `cd '${root.repoDir}' || exit 0
dir='${root.job?.dir ?? ""}'
install="$dir/${root.job?.install ?? ""}"
[ -x "$install" ] || exit 0

# Someone who removed a component on purpose gets to keep it removed.
optout='${root.job?.optOut ?? ""}'
if [ -n "$optout" ] && [ -f '${Paths.state}/'"$optout" ]; then
    exit 0
fi

for tool in ${(root.job?.requires ?? []).join(" ")}; do
    command -v "$tool" >/dev/null 2>&1 || exit 0
done

# The directory's own tree hash: it moves when that component's files move and
# stays put when the rest of the release changes around it.
tree=$(git rev-parse "HEAD:$dir" 2>/dev/null) || exit 0
stamp='${root.stampDir}/${root.job?.name ?? "unknown"}'
if [ "$(cat "$stamp" 2>/dev/null)" = "$tree" ]; then
    exit 0
fi

mkdir -p '${root.stampDir}'
"$install" >/dev/null 2>&1 || exit 1
printf '%s' "$tree" > "$stamp"
echo BUILT`]

        onExited: code => {
            if (code !== 0)
                root.lastError = qsTr("Could not build %1 — the shell's own is still in use").arg(root.job?.label ?? root.job?.name ?? qsTr("a component"));
            root.runNextComponent();
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
