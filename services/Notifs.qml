pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Notifications
import Caelestia
import Caelestia.Config
import qs.components.misc
import qs.services
import qs.utils

// Notifications, and who serves them.
//
// With the bar installed (bar/, a Tauri app) the bar is the notification
// server: it owns org.freedesktop.Notifications, keeps the history and draws
// both the toasts and the notification centre, for the same reason it draws
// the bar and the launcher. This singleton then stands down to a client. It
// holds no bus name and touches no file; it listens to the bar over a socket
// and mirrors what it is told into the same `list` the lock screen has
// always read. The lock screen is the one place the bar cannot draw: a
// session lock surface sits above every layer surface there is.
//
// Without the bar this is the whole server, exactly as it always was, so a
// checkout with no Rust toolchain still has notifications and
// `bar/install.sh --uninstall` gives them back.
Singleton {
    id: root

    // Unknown until ExternalBar's checks have answered. Nothing below claims
    // the bus name or reads the history before then: guessing wrong for a
    // moment would mean two servers fighting over one name and one file.
    //
    // A bar being installed is not enough. It has to be one that serves, which
    // the bar built before this one does not: see `servesNotifs`. That can
    // turn true while the shell is running, when an update finishes building
    // the bar, and the handover then happens on the spot. The bar is already
    // queued for the bus name by then, so it takes it the moment this lets go.
    readonly property bool decided: ExternalBar.ready && ExternalBar.notifsKnown
    readonly property bool external: ExternalBar.external && ExternalBar.servesNotifs
    readonly property bool bridged: bridgeLoader.item?.connected ?? false
    readonly property string bridgePath: `${Quickshell.env("XDG_RUNTIME_DIR") || "/tmp"}/caelestia-notifs.sock`

    property list<NotifData> list: []
    // Newest entries kept in the history; anything older is dropped on load
    // and closed as new ones arrive. Restoring a thousand entries froze the
    // shell for seconds and every save rewrote hundreds of kilobytes.
    readonly property int maxHistory: 300
    readonly property list<NotifData> notClosed: list.filter(n => !n.closed)
    readonly property list<NotifData> popups: list.filter(n => n.popup)
    property alias dnd: props.dnd

    property bool loaded

    function hasFullscreen(): bool {
        for (const monitor of Hypr.monitors.values) {
            if (monitor?.activeWorkspace?.toplevels.values.some(t => t.lastIpcObject.fullscreen > 1))
                return true;
        }
        return false;
    }

    function shouldShowPopup(): bool {
        if (props.dnd || ShellState.anySidebarOpen())
            return false;
        if (GlobalConfig.notifs.fullscreen === "off" && hasFullscreen())
            return false;
        return true;
    }

    // True while a feed from the bar is being applied, so that mirroring its
    // state is not mistaken for somebody flipping the switch here and sent
    // straight back.
    property bool mirroring

    onDndChanged: {
        if (external && !mirroring)
            tell(`dnd ${dnd ? "on" : "off"}`);

        if (!GlobalConfig.utilities.toasts.dndChanged)
            return;

        if (dnd)
            Toaster.toast(qsTr("Do not disturb enabled"), qsTr("Popup notifications are now disabled"), "do_not_disturb_on");
        else
            Toaster.toast(qsTr("Do not disturb disabled"), qsTr("Popup notifications are now enabled"), "do_not_disturb_off");
    }

    onListChanged: {
        // The bar keeps the history while it is the server. Two writers on
        // one file is how a history gets replaced by half of itself.
        if (loaded && !external)
            saveTimer.restart();
    }

    Timer {
        id: saveTimer

        interval: 1000
        // Checked again here and not only where it is started: the handover
        // to the bar can land inside the second this waits.
        onTriggered: root.external || storage.setText(JSON.stringify(root.notClosed.map(n => ({
                    time: n.time,
                    id: n.id,
                    summary: n.summary,
                    body: n.body,
                    appIcon: n.appIcon,
                    appName: n.appName,
                    image: n.image,
                    expireTimeout: n.expireTimeout,
                    urgency: n.urgency,
                    resident: n.resident,
                    hasActionIcons: n.hasActionIcons,
                    actions: n.actions
                }))))
    }

    PersistentProperties {
        id: props

        property bool dnd

        reloadableId: "notifs"
    }

    // This is the user's main shell — it owns notifications, full stop. The
    // server lives in a Loader so it can be rebuilt to (re)bind the D-Bus
    // name: if a foreign daemon (dunst/mako/swaync…) grabbed
    // org.freedesktop.Notifications first, the grabber below stops it and
    // rebuilds this, binding the freed name. quickshell then holds the name,
    // so a respawning daemon can't take it back. No manual "reclaim" needed.
    Loader {
        id: serverLoader

        active: root.decided && !root.external

        sourceComponent: NotificationServer {
            keepOnReload: false
            actionsSupported: true
            bodyHyperlinksSupported: true
            bodyImagesSupported: true
            bodyMarkupSupported: true
            imageSupported: true
            persistenceSupported: true

            onNotification: notif => {
                notif.tracked = true;

                const comp = notifComp.createObject(root, {
                    popup: root.shouldShowPopup(),
                    notification: notif
                });
                root.list = [comp, ...root.list];
                root.trimHistory();
            }
        }
    }

    // One stored or mirrored notification as the properties NotifData has.
    //
    // Two servers have written this shape. The bar's has a number for the
    // time and keys this object has no property for (`id`, `progress`,
    // `transient`), and handing those to createObject as they come is a
    // warning per key per notification, three hundred times over.
    function known(notif: var): var {
        return {
            notificationId: String(notif.notificationId ?? notif.id ?? ""),
            time: new Date(notif.time),
            summary: notif.summary ?? "",
            body: notif.body ?? "",
            appIcon: notif.appIcon ?? "",
            appName: notif.appName ?? "",
            image: notif.image ?? "",
            expireTimeout: notif.expireTimeout ?? GlobalConfig.notifs.defaultExpireTimeout,
            urgency: notif.urgency ?? NotificationUrgency.Normal,
            resident: notif.resident ?? false,
            hasActionIcons: notif.hasActionIcons ?? false,
            actions: notif.actions ?? [],
            // Never a popup here: a restored one is history, and a mirrored
            // one is already on screen, drawn by the bar.
            popup: false
        };
    }

    // An icon as something an Image can load. The bar resolves icon names to
    // files before it says anything, so what arrives from it is a path, which
    // the icon provider has no theme entry for.
    function iconSource(icon: string): string {
        return icon.startsWith("/") ? `file://${icon}` : Quickshell.iconPath(icon);
    }

    function clear(): void {
        if (external) {
            tell("clear");
            return;
        }
        for (const notif of root.list.slice())
            notif.close();
    }

    // The notification centre is the bar's while the bar is the server. The
    // shell still hears the keybind and still watches the screen's corner, so
    // it still has to be able to ask.
    property bool centreOpen

    function toggleCentre(): void {
        tell("centre toggle");
    }

    function openCentre(): void {
        if (!centreOpen)
            tell("centre open");
    }

    // One command to the bar. Dropped while the bar is still coming up: there
    // is nothing here worth queueing for a server that has not started.
    function tell(line: string): void {
        if (root.bridged)
            bridgeLoader.item.write(`${line}\n`);
    }

    // Makes `list` say what the bar says.
    //
    // The objects the lock screen is already drawing are kept and updated in
    // place rather than rebuilt, so a notification that is still there does
    // not blink, and one that has gone is closed the way it always was, which
    // is what lets its delegate animate out before it is destroyed.
    function mirror(line: string): void {
        let feed;
        try {
            feed = JSON.parse(line);
        } catch (e) {
            console.warn(`Notifs: the bar sent something that is not JSON: ${e}`);
            return;
        }

        mirroring = true;
        props.dnd = feed.dnd ?? false;
        mirroring = false;
        centreOpen = (feed.centre ?? "") !== "";

        const incoming = new Map((feed.list ?? []).map(n => [String(n.id), n]));
        const kept = [];
        // A copy, because close() takes things off the list being walked.
        for (const existing of root.list.slice()) {
            // Already on its way out, and held only by the delegate that is
            // animating it away. It is not a match for anything new, even if
            // the bar has reused its id.
            if (existing.closed)
                continue;

            const now = incoming.get(existing.notificationId);
            if (!now) {
                existing.close();
                continue;
            }
            incoming.delete(existing.notificationId);
            Object.assign(existing, root.known(now));
            kept.push(existing);
        }

        // close() leaves a notification on the list for as long as a delegate
        // holds it, and takes it off (and destroys it) when that lets go.
        // Dropping those here instead would orphan them: never on the list
        // again, so never destroyed.
        const leaving = root.list.filter(n => n.closed);
        const fresh = [...incoming.values()].map(n => notifComp.createObject(root, root.known(n)));
        root.list = kept.concat(fresh, leaving).sort((a, b) => b.time - a.time);
    }

    // A Quickshell Socket does not try again after a failed connect, so it
    // lives in a Loader that is rebuilt until a fresh one connects. The bar
    // is restarted by every install, and a bridge that never came back would
    // leave the lock screen showing the notifications of an hour ago.
    Loader {
        id: bridgeLoader

        active: root.external

        sourceComponent: Component {
            Socket {
                path: root.bridgePath
                connected: true

                parser: SplitParser {
                    splitMarker: "\n"
                    onRead: line => root.mirror(line)
                }
            }
        }
    }

    // Runs for as long as the bridge is down, rather than being started by
    // the socket saying it dropped. A first attempt that is refused (the bar
    // comes up a moment after the shell does) was never connected, so it
    // never reports a change, and a retry that waited to be told would wait
    // for ever. Quick at first, then backing off, as Firewall.qml does: a bar
    // that is installed and will not start must not have a socket rebuilt
    // under it every two seconds for the life of the shell.
    Timer {
        id: bridgeRetry

        property int backoffMs: 1000

        interval: backoffMs
        running: root.external && !root.bridged
        repeat: true
        onRunningChanged: {
            if (running)
                backoffMs = 1000;
        }
        onTriggered: {
            backoffMs = Math.min(backoffMs * 2, 30000);
            bridgeLoader.active = false;
            bridgeKick.restart();
        }
    }

    Timer {
        id: bridgeKick

        interval: 50
        onTriggered: bridgeLoader.active = Qt.binding(() => root.external)
    }

    // Asked again whenever the answer might have changed, so that uninstalling
    // the bar hands the bus name back without a restart.
    onExternalChanged: {
        if (external)
            return;
        // What is on the list was mirrored from the bar, and the history on
        // disk is about to be loaded on top of it.
        for (const notif of root.list.slice())
            notif.close();
    }

    function trimHistory(): void {
        const kept = root.notClosed;
        for (const stale of kept.slice(root.maxHistory))
            stale.close();
    }

    function _rebindServer(): void {
        serverLoader.active = false;
        Qt.callLater(() => serverLoader.active = true);
    }

    // Stops whoever owns the notification service if it isn't us, so the
    // server can claim it. Exits 10 when it displaced a competitor (rebind
    // needed), 0 when we already own it / it's free / nobody could be found.
    Process {
        id: grabber

        command: ["sh", "-c", `owner=$(busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus GetNameOwner s org.freedesktop.Notifications 2>/dev/null | awk '{print $2}' | tr -d '"')
[ -z "$owner" ] && exit 0
pid=$(busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus GetConnectionUnixProcessID s "$owner" 2>/dev/null | awk '{print $2}')
[ -z "$pid" ] && exit 0
[ "$pid" = "${Quickshell.processId}" ] && exit 0
# Displacing a notification daemon (dunst/mako/swaync/…) is the whole point,
# but some desktops hand this name to the session shell itself. Killing that
# takes the entire desktop down, so a compositor/session process is never a
# target — no matter what the cgroup below says. Nor is the bar: it serves
# notifications itself now, and if it holds the name while this server is the
# one running, the answer is for this one to let it, not to kill the bar every
# five minutes and have the shell start it again.
comm=$(cat "/proc/$pid/comm" 2>/dev/null)
case "$comm" in
    Hyprland|sway|river|niri|labwc|weston|plasmashell|kwin_wayland|kwin_x11|gnome-shell|xfce4-session|cinnamon-session|mate-session|lxqt-session|systemd|init|caelestia-bar|cae-shell)
        echo "refusing to displace session process $comm (pid $pid)" >&2
        exit 0 ;;
esac
# A daemon started from the compositor shares its cgroup unit; never stop
# that (it would kill the desktop) — only a dedicated notification unit.
unit=$(grep -oE '[a-zA-Z0-9@._-]+\\.service' "/proc/$pid/cgroup" 2>/dev/null | grep -viE 'user@|wayland-wm|graphical-session|hyprland|plasma|gnome-session|session\\.slice|init\\.scope' | head -1)
[ -n "$unit" ] && systemctl --user stop "$unit" 2>/dev/null
kill "$pid" 2>/dev/null
sleep 1
kill -9 "$pid" 2>/dev/null
exit 10`]

        onExited: code => {
            if (code === 10)
                root._rebindServer();
        }
    }

    // Grab at startup (after the server's own first bind attempt) and re-check
    // periodically — a no-op once we hold the name.
    // Only while this is the server. The bar takes the name itself, and
    // displaces whoever else holds it, when it is the one serving.
    Timer {
        running: root.decided && !root.external
        interval: 2000
        onTriggered: grabber.running = true
    }

    Timer {
        running: root.decided && !root.external
        repeat: true
        interval: 300000
        onTriggered: grabber.running = true
    }

    FileView {
        id: storage

        printErrors: false
        // No path, no load and no save: the bar owns this file while it is
        // the server.
        path: root.decided && !root.external ? `${Paths.state}/notifs.json` : ""
        onLoaded: {
            let data;
            try {
                data = JSON.parse(text());
            } catch (e) {
                console.warn(`Notifs: ${storage.path} is not valid JSON, starting with an empty history: ${e}`);
                data = [];
            }
            // Build the array first and assign once: pushing into the list
            // property per entry re-ran every filter and the sidebar's model
            // diff each time, quadratic in the history length
            data.sort((a, b) => new Date(b.time) - new Date(a.time));
            const restored = data.slice(0, root.maxHistory).map(notif => {
                return notifComp.createObject(root, root.known(notif));
            });
            root.list = root.list.concat(restored).sort((a, b) => b.time - a.time);
            root.loaded = true;
            // Shrink the file now rather than at the next notification
            if (data.length > root.maxHistory)
                saveTimer.restart();
        }
        onLoadFailed: err => {
            if (err === FileViewError.FileNotFound) {
                root.loaded = true;
                Qt.callLater(() => setText("[]"));
            }
        }
    }

    // qmllint disable unresolved-type
    CustomShortcut {
        // qmllint enable unresolved-type
        name: "clearNotifs"
        description: "Clear all notifications"
        onPressed: root.clear()
    }

    IpcHandler {
        function clear(): void {
            root.clear();
        }

        function isDndEnabled(): bool {
            return props.dnd;
        }

        function toggleDnd(): void {
            props.dnd = !props.dnd;
        }

        function enableDnd(): void {
            props.dnd = true;
        }

        function disableDnd(): void {
            props.dnd = false;
        }

        target: "notifs"
    }

    Component {
        id: notifComp

        NotifData {}
    }
}
